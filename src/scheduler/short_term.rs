use crate::{
    Card, Parameters,
    Rating::{self, *},
    State::{self, *},
};
use chrono::{DateTime, Duration, Utc};

pub struct ShortTerm {
    pub now: DateTime<Utc>,
    pub parameters: Parameters,
    pub card: Card,
}

impl ShortTerm {
    pub fn new(parameters: Parameters, card: Card, now: DateTime<Utc>) -> Self {
        Self {
            parameters,
            card,
            now,
        }
    }

    pub fn next_card(&self, rating: Rating) -> Card {
        let mut out = match self.card.state {
            New => self.review_new(rating),
            Learning | Relearning => self.review_learning(rating),
            Reviewing => self.review_reviewing(rating),
        };
        out.rating = rating;
        out
    }

    fn review_new(&self, rating: Rating) -> Card {
        let p = &self.parameters;

        let mut card = Card {
            difficulty: p.init_difficulty(rating),
            stability: p.init_stability(rating),
            reviewed_at: self.now,
            ..self.card
        };

        let (due, state) = match rating {
            Again => (Duration::minutes(1), Learning),
            Hard => (Duration::minutes(5), Learning),
            Good => (Duration::minutes(10), Learning),
            Easy => {
                let easy_interval = p.next_interval(card.stability) as i64;
                (Duration::days(easy_interval), Reviewing)
            }
        };

        card.due = self.now + due;
        card.state = state;
        card
    }

    fn review_learning(&self, rating: Rating) -> Card {
        let p = &self.parameters;
        let last = &self.card;

        let mut card = Card {
            difficulty: p.next_difficulty(last.difficulty, rating),
            stability: p.short_term_stability(last.stability, rating),
            reviewed_at: self.now,
            ..self.card
        };

        let (due, state) = match rating {
            Again => (Duration::minutes(5), last.state),
            Hard => (Duration::minutes(10), last.state),
            Good => {
                let good_interval = p.next_interval(card.stability) as i64;
                (Duration::days(good_interval), Reviewing)
            }
            Easy => {
                let good_stability = p.short_term_stability(last.stability, Good);
                let good_interval = p.next_interval(good_stability);
                let easy_interval = p.next_interval(card.stability).max(good_interval + 1.0) as i64;
                (Duration::days(easy_interval), Reviewing)
            }
        };

        card.due = self.now + due;
        card.state = state;
        card
    }

    fn review_reviewing(&self, rating: Rating) -> Card {
        let p = &self.parameters;
        let stability = self.card.stability;
        let difficulty = self.card.difficulty;
        let retrievability = self.card.retrievability(p, self.now);

        let mut card = Card {
            difficulty: p.next_difficulty(difficulty, rating),
            stability: p.next_stability(difficulty, stability, retrievability, rating),
            reviewed_at: self.now,
            ..self.card
        };

        let interval = self.parameters.next_interval(card.stability);
        card.due = self.now
            + (match rating {
                Again => Duration::minutes(5),
                Hard | Good | Easy => Duration::days(interval as i64),
            });
        card.state = next_state(rating);
        card
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
        scheduler::short_term::ShortTerm,
        testing::{string_to_utc, RoundFloat, TEST_RATINGS, WEIGHTS},
        State,
    };
    use chrono::Duration;

    #[test]
    fn interval() {
        let mut card = Card::new();
        let mut now = string_to_utc("2022-11-29 12:30:00 +0000 UTC");
        let mut interval_history = vec![];

        for rating in TEST_RATINGS.into_iter() {
            let scheduler = ShortTerm::new(Parameters::default(), card, now);
            card = scheduler.next_card(rating);
            interval_history.push(card.scheduled_days());
            now = card.due;
        }
        let expected = [0, 4, 15, 48, 136, 351, 0, 0, 7, 13, 24, 43, 77];
        assert_eq!(interval_history, expected);
    }

    #[test]
    fn state() {
        let params = Parameters {
            w: WEIGHTS,
            ..Default::default()
        };

        let mut card = Card::new();
        let mut now = string_to_utc("2022-11-29 12:30:00 +0000 UTC");
        let mut state_list = vec![];

        for rating in TEST_RATINGS.into_iter() {
            state_list.push(card.state);
            let scheduler = ShortTerm::new(params, card, now);
            card = scheduler.next_card(rating);
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
    fn memo_state() {
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
            card = ShortTerm::new(params, card, now).next_card(rating);
            now += Duration::days(intervals[index] as i64);
        }

        card = ShortTerm::new(params, card, now).next_card(Rating::Good);
        assert_eq!(card.stability.round_float(4), 71.4554);
        assert_eq!(card.difficulty.round_float(4), 5.0976);
    }

    #[test]
    fn retrievability() {
        let card = Card::new();
        let now = string_to_utc("2022-11-29 12:30:00 +0000 UTC");
        let expect_retrievability = [1.0, 1.0, 1.0, 0.9026208];

        for (i, rating) in [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy]
            .into_iter()
            .enumerate()
        {
            let card = ShortTerm::new(Parameters::default(), card, now).next_card(rating);
            let retrievability = card.retrievability(&Parameters::default(), card.due);

            assert_eq!(retrievability.round_float(7), expect_retrievability[i]);
        }
    }
}