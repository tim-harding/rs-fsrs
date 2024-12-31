use super::base::Base;
use crate::{
    Card, Parameters,
    Rating::{self, *},
    SchedulingInfo,
    State::*,
};
use chrono::{DateTime, Duration, Utc};

pub struct Longterm(Base);

impl Longterm {
    pub fn new(parameters: Parameters, card: Card, now: DateTime<Utc>) -> Self {
        Self {
            0: Base::new(parameters, card, now),
        }
    }

    fn new_state(&mut self, rating: Rating) -> SchedulingInfo {
        if let Some(exist) = self.0.next.get(&rating) {
            return exist.clone();
        }

        let next = self.0.current.clone();
        self.0.current.scheduled_days = 0;
        self.0.current.elapsed_days = 0;

        let mut next_again = next.clone();
        let mut next_hard = next.clone();
        let mut next_good = next.clone();
        let mut next_easy = next;

        self.init_difficulty_stability(
            &mut next_again,
            &mut next_hard,
            &mut next_good,
            &mut next_easy,
        );
        self.next_interval(
            &mut next_again,
            &mut next_hard,
            &mut next_good,
            &mut next_easy,
            0,
        );
        self.next_state(
            &mut next_again,
            &mut next_hard,
            &mut next_good,
            &mut next_easy,
        );
        self.update_next(&next_again, &next_hard, &next_good, &next_easy);

        self.0.next.get(&rating).unwrap().to_owned()
    }

    fn learning_state(&mut self, rating: Rating) -> SchedulingInfo {
        self.review_state(rating)
    }

    fn review_state(&mut self, rating: Rating) -> SchedulingInfo {
        if let Some(exist) = self.0.next.get(&rating) {
            return exist.clone();
        }

        let next = self.0.current.clone();
        let interval = self.0.current.elapsed_days;
        let stability = self.0.last.stability;
        let difficulty = self.0.last.difficulty;
        let retrievability = self.0.last.retrievability(self.0.now);

        let mut next_again = next.clone();
        let mut next_hard = next.clone();
        let mut next_good = next.clone();
        let mut next_easy = next;

        self.next_difficulty_stability(
            &mut next_again,
            &mut next_hard,
            &mut next_good,
            &mut next_easy,
            difficulty,
            stability,
            retrievability,
        );
        self.next_interval(
            &mut next_again,
            &mut next_hard,
            &mut next_good,
            &mut next_easy,
            interval,
        );
        self.next_state(
            &mut next_again,
            &mut next_hard,
            &mut next_good,
            &mut next_easy,
        );
        next_again.lapses += 1;

        self.update_next(&next_again, &next_hard, &next_good, &next_easy);
        self.0.next.get(&rating).unwrap().to_owned()
    }

    fn init_difficulty_stability(
        &self,
        next_again: &mut Card,
        next_hard: &mut Card,
        next_good: &mut Card,
        next_easy: &mut Card,
    ) {
        next_again.difficulty = self.0.parameters.init_difficulty(Again);
        next_again.stability = self.0.parameters.init_stability(Again);

        next_hard.difficulty = self.0.parameters.init_difficulty(Hard);
        next_hard.stability = self.0.parameters.init_stability(Hard);

        next_good.difficulty = self.0.parameters.init_difficulty(Good);
        next_good.stability = self.0.parameters.init_stability(Good);

        next_easy.difficulty = self.0.parameters.init_difficulty(Easy);
        next_easy.stability = self.0.parameters.init_stability(Easy);
    }

    #[allow(clippy::too_many_arguments)]
    fn next_difficulty_stability(
        &self,
        next_again: &mut Card,
        next_hard: &mut Card,
        next_good: &mut Card,
        next_easy: &mut Card,
        difficulty: f64,
        stability: f64,
        retrievability: f64,
    ) {
        next_again.difficulty = self.0.parameters.next_difficulty(difficulty, Again);
        next_again.stability =
            self.0
                .parameters
                .next_forget_stability(difficulty, stability, retrievability);

        next_hard.difficulty = self.0.parameters.next_difficulty(difficulty, Hard);
        next_hard.stability =
            self.0
                .parameters
                .next_recall_stability(difficulty, stability, retrievability, Hard);

        next_good.difficulty = self.0.parameters.next_difficulty(difficulty, Good);
        next_good.stability =
            self.0
                .parameters
                .next_recall_stability(difficulty, stability, retrievability, Good);

        next_easy.difficulty = self.0.parameters.next_difficulty(difficulty, Easy);
        next_easy.stability =
            self.0
                .parameters
                .next_recall_stability(difficulty, stability, retrievability, Easy);
    }

    fn next_interval(
        &self,
        next_again: &mut Card,
        next_hard: &mut Card,
        next_good: &mut Card,
        next_easy: &mut Card,
        elapsed_days: i64,
    ) {
        let mut again_interval = self
            .0
            .parameters
            .next_interval(next_again.stability, elapsed_days);
        let mut hard_interval = self
            .0
            .parameters
            .next_interval(next_hard.stability, elapsed_days);
        let mut good_interval = self
            .0
            .parameters
            .next_interval(next_good.stability, elapsed_days);
        let mut easy_interval = self
            .0
            .parameters
            .next_interval(next_easy.stability, elapsed_days);

        again_interval = again_interval.min(hard_interval);
        hard_interval = hard_interval.max(again_interval + 1.0);
        good_interval = good_interval.max(hard_interval + 1.0);
        easy_interval = easy_interval.max(good_interval + 1.0);

        next_again.scheduled_days = again_interval as i64;
        next_again.due = self.0.now + Duration::days(again_interval as i64);

        next_hard.scheduled_days = hard_interval as i64;
        next_hard.due = self.0.now + Duration::days(hard_interval as i64);

        next_good.scheduled_days = good_interval as i64;
        next_good.due = self.0.now + Duration::days(good_interval as i64);

        next_easy.scheduled_days = easy_interval as i64;
        next_easy.due = self.0.now + Duration::days(easy_interval as i64);
    }

    fn next_state(
        &self,
        next_again: &mut Card,
        next_hard: &mut Card,
        next_good: &mut Card,
        next_easy: &mut Card,
    ) {
        next_again.state = Review;
        next_hard.state = Review;
        next_good.state = Review;
        next_easy.state = Review;
    }

    fn update_next(
        &mut self,
        next_again: &Card,
        next_hard: &Card,
        next_good: &Card,
        next_easy: &Card,
    ) {
        let item_again = SchedulingInfo {
            card: next_again.clone(),
            review_log: self.0.build_log(Again),
        };
        let item_hard = SchedulingInfo {
            card: next_hard.clone(),
            review_log: self.0.build_log(Hard),
        };
        let item_good = SchedulingInfo {
            card: next_good.clone(),
            review_log: self.0.build_log(Good),
        };
        let item_easy = SchedulingInfo {
            card: next_easy.clone(),
            review_log: self.0.build_log(Easy),
        };

        self.0.next.insert(Again, item_again);
        self.0.next.insert(Hard, item_hard);
        self.0.next.insert(Good, item_good);
        self.0.next.insert(Easy, item_easy);
    }

    pub fn review(&mut self, rating: Rating) -> SchedulingInfo {
        match self.0.last.state {
            New => self.new_state(rating),
            Learning | Relearning => self.learning_state(rating),
            Review => self.review_state(rating),
        }
    }
}
