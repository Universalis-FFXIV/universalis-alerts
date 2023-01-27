use std::fmt::{Display, Formatter};

use crate::universalis::*;
use itertools::Itertools;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
enum TriggerFilter {
    #[serde(rename = "hq")]
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

impl Display for TriggerFilter {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self {
            Self::Hq => f.write_str("Item is HQ"),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
enum TriggerMapper {
    #[serde(rename = "pricePerUnit")]
    UnitPrice,
}

trait TriggerMapOp<TItem, TResult> {
    fn evaluate(&self, item: &TItem) -> TResult;
}

impl TriggerMapOp<Listing, f32> for TriggerMapper {
    fn evaluate(&self, listing: &Listing) -> f32 {
        match self {
            Self::UnitPrice => listing.unit_price as f32,
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
enum TriggerReducer {
    #[serde(rename = "min")]
    Min,
    #[serde(rename = "max")]
    Max,
    #[serde(rename = "mean")]
    Mean,
}

struct ReducerContext<T> {
    stack: Vec<T>,
}

trait TriggerReduceOp<T> {
    fn evaluate(&self, context: &mut ReducerContext<T>, accum: &T, item: &T) -> T;
}

impl TriggerReduceOp<f32> for TriggerReducer {
    fn evaluate(&self, context: &mut ReducerContext<f32>, accum: &f32, item: &f32) -> f32 {
        match self {
            Self::Min => (*accum).min(*item),
            Self::Max => (*accum).max(*item),
            Self::Mean => {
                // The initial accumulator value is the first element
                // of the iterator, so n should begin at 1.
                let n = context.stack.pop().unwrap_or(1.0);
                context.stack.push(n + 1.0);
                (n * *accum + *item) / (n + 1.0)
            }
        }
    }
}

impl Display for TriggerReducer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self {
            Self::Min => f.write_str("Min"),
            Self::Max => f.write_str("Max"),
            Self::Mean => f.write_str("Mean"),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
enum Comparison {
    #[serde(rename = "lt")]
    LessThan { target: f32 },
    #[serde(rename = "gt")]
    GreaterThan { target: f32 },
}

trait ComparisonOp<T> {
    fn evaluate(&self, value: &T) -> bool;
}

impl ComparisonOp<f32> for Comparison {
    fn evaluate(&self, value: &f32) -> bool {
        match self {
            Self::LessThan { target } => *value < *target,
            Self::GreaterThan { target } => *value > *target,
        }
    }
}

impl Display for Comparison {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self {
            Self::LessThan { target } => f.write_fmt(format_args!("Less than {}", target)),
            Self::GreaterThan { target } => f.write_fmt(format_args!("Greater than {}", target)),
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
    pub fn evaluate(&self, listings: &[Listing]) -> Option<f32> {
        let mut context = ReducerContext::<f32> { stack: Vec::new() };
        listings
            .into_iter()
            // Execute all filters on each listing
            .filter(|l| self.filters.iter().all(|f| f.evaluate(l)))
            // Map each listing to a scalar
            .map(|l| self.mapper.evaluate(l))
            // Execute the specified reducer
            .reduce(|accum, item| self.reducer.evaluate(&mut context, &accum, &item))
            // Check if the result satisfies the final comparison
            .filter(|result| self.comparison.evaluate(result))
    }
}

impl Display for AlertTrigger {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        let formatted_filters = self.filters.iter().map(|filter| format!("{}", filter));
        let formatted_filters =
            Itertools::intersperse(formatted_filters, "\n".to_string()).collect::<String>();
        f.write_fmt(format_args!(
            "{}\n\nField: {}\nStat: {}\nComparison: {}",
            formatted_filters, self.mapper, self.reducer, self.comparison
        ))
    }
}
