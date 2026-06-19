use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyActionSellCountInput {
    pub size: i64,
    pub side: String,
    pub counts_as_executed: bool,
}

#[must_use]
pub fn executed_sell_offer_counts_by_size(
    action_items: &[StrategyActionSellCountInput],
) -> BTreeMap<i64, i64> {
    let mut counts = BTreeMap::default();
    for item in action_items {
        if !item.counts_as_executed {
            continue;
        }
        if item.side != "sell" {
            continue;
        }
        if item.size <= 0 {
            continue;
        }
        *counts.entry(item.size).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::{executed_sell_offer_counts_by_size, StrategyActionSellCountInput};

    #[test]
    fn executed_counts_only_executed_sell_items() {
        let items = vec![
            StrategyActionSellCountInput {
                size: 10,
                side: "sell".to_string(),
                counts_as_executed: true,
            },
            StrategyActionSellCountInput {
                size: 10,
                side: "sell".to_string(),
                counts_as_executed: true,
            },
            StrategyActionSellCountInput {
                size: 10,
                side: "buy".to_string(),
                counts_as_executed: true,
            },
            StrategyActionSellCountInput {
                size: 10,
                side: "sell".to_string(),
                counts_as_executed: false,
            },
            StrategyActionSellCountInput {
                size: 1,
                side: "sell".to_string(),
                counts_as_executed: true,
            },
        ];
        let got = executed_sell_offer_counts_by_size(&items);
        assert_eq!(got.get(&10), Some(&2));
        assert_eq!(got.get(&1), Some(&1));
    }

    #[test]
    fn executed_counts_includes_pending_visibility_sells() {
        let items = vec![
            StrategyActionSellCountInput {
                size: 10,
                side: "sell".to_string(),
                counts_as_executed: true,
            },
            StrategyActionSellCountInput {
                size: 10,
                side: "sell".to_string(),
                counts_as_executed: false,
            },
        ];
        let got = executed_sell_offer_counts_by_size(&items);
        assert_eq!(got.get(&10), Some(&1));
    }
}
