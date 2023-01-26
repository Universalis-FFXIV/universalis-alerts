use std::fmt::{Display, Formatter};

use crate::universalis::*;
use itertools::Itertools;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub enum TriggerFilter {
    Hq,
}

trait TriggerFilterOp<T> {
    fn evaluate(&self, value: &T) -> bool;
}

impl TriggerFilterOp<Listing> for TriggerFilter {
    fn evaluate(&self, value: &Listing) -> bool {
        match self {
            Self::Hq => value.hq,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub enum TriggerMapper {
    UnitPrice,
}

trait TriggerMapOp<TItem, TResult> {
    fn evaluate(&self, item: &TItem) -> TResult;
}

impl TriggerMapOp<Listing, i32> for TriggerMapper {
    fn evaluate(&self, listing: &Listing) -> i32 {
        match self {
            Self::UnitPrice => listing.unit_price,
        }
    }
}

impl Display for TriggerMapper {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self {
            Self::UnitPrice => f.write_str("Unit price"),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub enum TriggerReducer {
    Min,
}

trait TriggerReduceOp<T> {
    fn evaluate(&self, accum: &T, item: &T) -> T;
}

impl TriggerReduceOp<i32> for TriggerReducer {
    fn evaluate(&self, accum: &i32, item: &i32) -> i32 {
        match self {
            Self::Min => (*accum).min(*item),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub enum Comparison {
    LessThan { target: i32 },
}

trait ComparisonOp<T> {
    fn evaluate(&self, value: &T) -> bool;
}

impl ComparisonOp<i32> for Comparison {
    fn evaluate(&self, value: &i32) -> bool {
        match self {
            Self::LessThan { target } => *value < *target,
        }
    }
}

impl Display for Comparison {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self {
            Self::LessThan { target } => f.write_fmt(format_args!("Less than {}", target)),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct AlertTrigger {
    filters: Vec<TriggerFilter>,
    mapper: TriggerMapper,
    reducer: TriggerReducer,
    comparison: Comparison,
}

impl AlertTrigger {
    pub fn evaluate(&self, listings: &[Listing]) -> Option<i32> {
        listings
            .into_iter()
            // Execute all filters on each listing
            .filter(|l| self.filters.clone().into_iter().all(|f| f.evaluate(l)))
            // Map each listing to a scalar
            .map(|l| self.mapper.evaluate(l))
            // Execute the specified reducer
            .reduce(|accum, item| self.reducer.evaluate(&accum, &item))
            // Check if the result satisfies the final comparison
            .filter(|result| self.comparison.evaluate(result))
    }
}

impl Display for AlertTrigger {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        let formatted_trigger = self.filters.clone().into_iter().map(|filter| match filter {
            TriggerFilter::Hq => "Item is HQ".to_string(),
        });
        let formatted_trigger =
            Itertools::intersperse(formatted_trigger, "\n".to_string()).collect::<String>();
        f.write_fmt(format_args!(
            "{}\n\nField: {}\nComparison: {}",
            formatted_trigger, self.mapper, self.comparison
        ))
    }
}
