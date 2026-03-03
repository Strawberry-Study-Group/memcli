use crate::config::WeightConfig;

/// Apply positive feedback (boost): additive increase
///   weight = min(weight + boost_amount, 1.0)
pub fn boost(current_weight: f32, config: &WeightConfig) -> f32 {
    (current_weight + config.boost_amount).min(1.0)
}

/// Apply negative feedback (penalize): multiplicative decrease
///   weight = weight × penalty_factor
pub fn penalize(current_weight: f32, config: &WeightConfig) -> f32 {
    current_weight * config.penalty_factor
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> WeightConfig {
        WeightConfig {
            boost_amount: 0.1,
            penalty_factor: 0.8,
            warn_threshold: 0.1,
        }
    }

    #[test]
    fn test_boost_caps_at_1() {
        let config = default_config();
        assert_eq!(boost(0.95, &config), 1.0);
        assert_eq!(boost(1.0, &config), 1.0);
    }

    #[test]
    fn test_boost_additive() {
        let config = default_config();
        let result = boost(0.5, &config);
        assert!((result - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn test_penalize_multiplicative() {
        let config = default_config();
        let result = penalize(1.0, &config);
        assert!((result - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn test_penalize_geometric_decay() {
        let config = default_config();
        let mut w = 1.0_f32;
        for _ in 0..10 {
            w = penalize(w, &config);
        }
        // After 10 penalties: 1.0 * 0.8^10 ≈ 0.107
        assert!(w > 0.10 && w < 0.12);
    }
}
