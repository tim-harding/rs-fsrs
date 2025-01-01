use super::base::Base;
use crate::{
    cards::Cards,
    Card, Parameters,
    Rating::{self, *},
    Review, Schedule,
    State::{self, *},
};
use chrono::{DateTime, Duration, Utc};

pub struct Basic(Base);

impl Basic {
    pub fn new(parameters: Parameters, card: Card, now: DateTime<Utc>) -> Self {
        Self(Base::new(parameters, card, now))
    }

    // TODO: Move this into Scheduler only
    pub fn schedule(&self, rating: Rating) -> Schedule {
        Schedule {
            card: self.next_card(rating),
            review: self.current_review(rating),
        }
    }

    pub fn next_card(&self, rating: Rating) -> Card {
        match self.0.last.state {
            New => self.review_new(rating),
            Learning | Relearning => self.review_learning(rating),
            Reviewing => self.review_reviewing(rating),
        }
    }

    pub const fn current_review(&self, rating: Rating) -> Review {
        self.0.current_review(rating)
    }

    fn review_new(&self, rating: Rating) -> Card {
        let p = &self.0.parameters;

        let mut card = self.0.current;
        card.difficulty = p.init_difficulty(rating);
        card.stability = p.init_stability(rating);

        let (days, due, state) = match rating {
            Again => (0, Duration::minutes(1), Learning),
            Hard => (0, Duration::minutes(5), Learning),
            Good => (0, Duration::minutes(10), Learning),
            Easy => {
                let easy_interval = p.next_interval(card.stability, card.elapsed_days) as i64;
                (easy_interval, Duration::days(easy_interval), Reviewing)
            }
        };

        card.scheduled_days = days;
        card.due = self.0.now + due;
        card.state = state;
        card
    }

    fn review_learning(&self, rating: Rating) -> Card {
        let p = &self.0.parameters;
        let interval = self.0.current.elapsed_days;

        let mut card = self.0.current;
        card.difficulty = p.next_difficulty(self.0.last.difficulty, rating);
        card.stability = p.short_term_stability(self.0.last.stability, rating);

        let (days, due, state) = match rating {
            Again => (0, Duration::minutes(5), self.0.last.state),
            Hard => (0, Duration::minutes(10), self.0.last.state),
            Good => {
                let good_interval = p.next_interval(card.stability, interval) as i64;
                (good_interval, Duration::days(good_interval), Reviewing)
            }
            Easy => {
                let good_stability = p.short_term_stability(self.0.last.stability, Good);
                let good_interval = p.next_interval(good_stability, interval);
                let easy_interval = p
                    .next_interval(card.stability, interval)
                    .max(good_interval + 1.0) as i64;
                (easy_interval, Duration::days(easy_interval), Reviewing)
            }
        };

        card.scheduled_days = days;
        card.due = self.0.now + due;
        card.state = state;
        card
    }

    fn review_reviewing(&self, rating: Rating) -> Card {
        let p = &self.0.parameters;
        let interval = self.0.current.elapsed_days;
        let stability = self.0.last.stability;
        let difficulty = self.0.last.difficulty;
        let retrievability = self.0.last.retrievability(p, self.0.now);

        let mut cards = Cards::new(self.0.current);
        cards.update(|(rating, card)| {
            card.difficulty = p.next_difficulty(difficulty, rating);
            card.stability = p.next_stability(difficulty, stability, retrievability, rating);
        });

        let [hard_interval, good_interval, easy_interval] = self.review_intervals(
            cards.hard.stability,
            cards.good.stability,
            cards.easy.stability,
            interval,
        );

        let (days, due, lapses) = match rating {
            Again => (0, Duration::minutes(5), 1),
            Hard => (hard_interval, Duration::days(hard_interval), 0),
            Good => (good_interval, Duration::days(good_interval), 0),
            Easy => (easy_interval, Duration::days(easy_interval), 0),
        };

        let mut card = cards.get(rating);
        card.scheduled_days = days;
        card.due = self.0.now + due;
        card.lapses += lapses;
        card.state = next_state(rating);
        card
    }

    fn review_intervals(
        &self,
        hard_stability: f64,
        good_stability: f64,
        easy_stability: f64,
        interval: i64,
    ) -> [i64; 3] {
        let p = &self.0.parameters;
        let hard_interval = p.next_interval(hard_stability, interval);
        let good_interval = p.next_interval(good_stability, interval);
        let hard_interval = hard_interval.min(good_interval);
        let good_interval = good_interval.max(hard_interval + 1.0);
        let easy_interval = p
            .next_interval(easy_stability, interval)
            .max(good_interval + 1.0);
        [
            hard_interval as i64,
            good_interval as i64,
            easy_interval as i64,
        ]
    }
}

fn next_state(rating: Rating) -> State {
    match rating {
        Again => Relearning,
        Hard | Good | Easy => Reviewing,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        models::{Card, Rating},
        parameters::Parameters,
        scheduler::basic::Basic,
        State,
    };
    use chrono::{DateTime, Duration, TimeZone, Utc};

    static TEST_RATINGS: [Rating; 13] = [
        Rating::Good,
        Rating::Good,
        Rating::Good,
        Rating::Good,
        Rating::Good,
        Rating::Good,
        Rating::Again,
        Rating::Again,
        Rating::Good,
        Rating::Good,
        Rating::Good,
        Rating::Good,
        Rating::Good,
    ];

    static WEIGHTS: [f64; 19] = [
        0.4197, 1.1869, 3.0412, 15.2441, 7.1434, 0.6477, 1.0007, 0.0674, 1.6597, 0.1712, 1.1178,
        2.0225, 0.0904, 0.3025, 2.1214, 0.2498, 2.9466, 0.4891, 0.6468,
    ];

    fn string_to_utc(date_string: &str) -> DateTime<Utc> {
        let datetime = DateTime::parse_from_str(date_string, "%Y-%m-%d %H:%M:%S %z %Z").unwrap();
        Utc.from_local_datetime(&datetime.naive_utc()).unwrap()
    }

    trait RoundFloat {
        fn round_float(self, precision: i32) -> f64;
    }

    impl RoundFloat for f64 {
        fn round_float(self, precision: i32) -> f64 {
            let multiplier = 10.0_f64.powi(precision);
            (self * multiplier).round() / multiplier
        }
    }

    #[test]
    fn test_basic_scheduler_interval() {
        let mut card = Card::new();
        let mut now = string_to_utc("2022-11-29 12:30:00 +0000 UTC");
        let mut interval_history = vec![];

        for rating in TEST_RATINGS.into_iter() {
            let scheduler = Basic::new(Parameters::default(), card, now);
            card = scheduler.next_card(rating);
            interval_history.push(card.scheduled_days);
            now = card.due;
        }
        let expected = [0, 4, 15, 48, 136, 351, 0, 0, 7, 13, 24, 43, 77];
        assert_eq!(interval_history, expected);
    }

    #[test]
    fn test_basic_scheduler_state() {
        let params = Parameters {
            w: WEIGHTS,
            ..Default::default()
        };

        let mut card = Card::new();
        let mut now = string_to_utc("2022-11-29 12:30:00 +0000 UTC");
        let mut state_list = vec![];

        for rating in TEST_RATINGS.into_iter() {
            let record = Basic::new(params.clone(), card, now).schedule(rating);
            card = record.card;
            let rev_log = record.review;
            state_list.push(rev_log.state);
            now = card.due;
        }
        use State::*;
        let expected = [
            New, Learning, Reviewing, Reviewing, Reviewing, Reviewing, Reviewing, Relearning,
            Relearning, Reviewing, Reviewing, Reviewing, Reviewing,
        ];
        assert_eq!(state_list, expected);
    }

    #[test]
    fn test_basic_scheduler_memo_state() {
        let params = Parameters {
            w: WEIGHTS,
            ..Default::default()
        };

        let mut card = Card::new();
        let mut now = string_to_utc("2022-11-29 12:30:00 +0000 UTC");
        let ratings = [
            Rating::Again,
            Rating::Good,
            Rating::Good,
            Rating::Good,
            Rating::Good,
            Rating::Good,
        ];
        let intervals = [0, 0, 1, 3, 8, 21];
        for (index, rating) in ratings.into_iter().enumerate() {
            card = Basic::new(params.clone(), card, now).next_card(rating);
            now += Duration::days(intervals[index] as i64);
        }

        card = Basic::new(params.clone(), card, now).next_card(Rating::Good);
        assert_eq!(card.stability.round_float(4), 71.4554);
        assert_eq!(card.difficulty.round_float(4), 5.0976);
    }

    #[test]
    fn test_get_retrievability() {
        let card = Card::new();
        let now = string_to_utc("2022-11-29 12:30:00 +0000 UTC");
        let expect_retrievability = [1.0, 1.0, 1.0, 0.9026208];

        for (i, rating) in [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy]
            .into_iter()
            .enumerate()
        {
            let card = Basic::new(Parameters::default(), card, now).next_card(rating);
            let retrievability = card.retrievability(&Parameters::new(), card.due);

            assert_eq!(retrievability.round_float(7), expect_retrievability[i]);
        }
    }
}
