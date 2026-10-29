//! Budget monitoring rules. No HTTP, OIDC or SMTP dependency in the rule itself.
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct Recipient {
    pub address: String,
    pub verified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetRule {
    pub threshold_cents: u64,
    notified: bool,
    recovery_pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notification {
    Exceeded,
    Recovered,
}

impl BudgetRule {
    pub fn new(threshold_cents: u64) -> Self {
        Self {
            threshold_cents,
            notified: false,
            recovery_pending: false,
        }
    }
    pub fn set_threshold(&mut self, threshold_cents: u64) {
        if self.threshold_cents != threshold_cents {
            self.threshold_cents = threshold_cents;
            self.notified = false;
            self.recovery_pending = false;
        }
    }
    pub fn notification(&mut self, total: u64) -> Option<Notification> {
        if total <= self.threshold_cents {
            self.recovery_pending |= self.notified;
            self.notified = false;
            return self.recovery_pending.then_some(Notification::Recovered);
        }
        // Never send an obsolete recovery if the budget is exceeded again.
        self.recovery_pending = false;
        (!self.notified).then_some(Notification::Exceeded)
    }
    // Acknowledge ONLY after successful SMTP delivery. Errors are retried.
    pub fn delivered(&mut self, notification: Notification) {
        match notification {
            Notification::Exceeded => self.notified = true,
            Notification::Recovered => self.recovery_pending = false,
        }
    }
}

#[derive(Default)]
pub struct BudgetRules {
    pub rules: BTreeMap<u64, BudgetRule>,
    instance: Option<String>,
}

impl BudgetRules {
    pub fn synchronize(&mut self, instance: &str, ids: &[u64]) {
        if self.instance.as_deref().is_some_and(|old| old != instance) {
            self.rules.clear();
        }
        self.instance = Some(instance.to_owned());
        self.rules.retain(|id, _| ids.contains(id));
    }
    pub fn configure(&mut self, id: u64, cents: u64) -> Result<(), &'static str> {
        if let Some(rule) = self.rules.get_mut(&id) {
            rule.set_threshold(cents);
        } else {
            if self.rules.len() >= 256 {
                return Err("Le nombre maximum de surveillances est atteint.");
            }
            self.rules.insert(id, BudgetRule::new(cents));
        }
        Ok(())
    }
}

pub fn parse_euros(value: &str) -> Result<u64, &'static str> {
    let invalid = "Saisis un seuil positif ou nul, avec au maximum deux décimales.";
    let value = value.trim();
    let parts: Vec<_> = value.split(['.', ',']).collect();
    if parts.is_empty()
        || parts.len() > 2
        || parts[0].is_empty()
        || !parts[0].bytes().all(|b| b.is_ascii_digit())
    {
        return Err(invalid);
    }
    let euros = parts[0].parse::<u64>().map_err(|_| invalid)?;
    let cents = match parts.get(1) {
        None => 0,
        Some(fraction)
            if !fraction.is_empty()
                && fraction.len() <= 2
                && fraction.bytes().all(|b| b.is_ascii_digit()) =>
        {
            fraction.parse::<u64>().map_err(|_| invalid)? * if fraction.len() == 1 { 10 } else { 1 }
        }
        _ => return Err(invalid),
    };
    euros
        .checked_mul(100)
        .and_then(|n| n.checked_add(cents))
        .ok_or(invalid)
}

pub fn money(cents: u64) -> String {
    format!("{},{:02} €", cents / 100, cents % 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_delivery_per_episode_and_equality_rearms() {
        let mut rule = BudgetRule::new(1000);
        assert_eq!(rule.notification(1000), None);
        assert_eq!(rule.notification(1001), Some(Notification::Exceeded));
        assert_eq!(rule.notification(1001), Some(Notification::Exceeded));
        rule.delivered(Notification::Exceeded);
        assert_eq!(rule.notification(5000), None);
        assert_eq!(rule.notification(1000), Some(Notification::Recovered));
        assert_eq!(rule.notification(900), Some(Notification::Recovered)); // Retry failed recovery.
        rule.delivered(Notification::Recovered);
        assert_eq!(rule.notification(900), None);
        assert_eq!(rule.notification(1001), Some(Notification::Exceeded));
    }
    #[test]
    fn changing_threshold_rearms_but_saving_identical_value_does_not_spam() {
        let mut rule = BudgetRule::new(1000);
        rule.delivered(Notification::Exceeded);
        rule.set_threshold(1000);
        assert_eq!(rule.notification(2000), None);
        rule.set_threshold(900);
        assert_eq!(rule.notification(2000), Some(Notification::Exceeded));
    }
    #[test]
    fn recovery_requires_a_delivered_alert_and_rebound_cancels_stale_recovery() {
        let mut rule = BudgetRule::new(1000);
        assert_eq!(rule.notification(2000), Some(Notification::Exceeded));
        assert_eq!(rule.notification(500), None); // No alert was delivered.
        rule.delivered(Notification::Exceeded);
        assert_eq!(rule.notification(500), Some(Notification::Recovered));
        assert_eq!(rule.notification(2000), Some(Notification::Exceeded));
        assert_eq!(rule.notification(500), None); // Rebound alert also failed.
    }
    #[test]
    fn changing_threshold_cancels_pending_recovery_without_a_false_normal_email() {
        let mut rule = BudgetRule::new(1000);
        rule.delivered(Notification::Exceeded);
        assert_eq!(rule.notification(500), Some(Notification::Recovered));
        rule.set_threshold(1000);
        assert_eq!(rule.notification(500), Some(Notification::Recovered));
        rule.set_threshold(2000);
        assert_eq!(rule.notification(500), None);
    }
    #[test]
    fn amounts_are_exact_and_invalid_or_overflowing_values_are_rejected() {
        for (value, cents) in [("0", 0), ("12,50", 1250), ("12.5", 1250), (" 25 ", 2500)] {
            assert_eq!(parse_euros(value), Ok(cents));
        }
        for value in [
            "",
            "-1",
            "NaN",
            "1e3",
            "1.234",
            "1,2,3",
            "1.",
            "1 000",
            "18446744073709551616",
        ] {
            assert!(parse_euros(value).is_err());
        }
    }
    #[test]
    fn removed_selections_and_reused_ids_after_backend_restart_cannot_inherit_rules() {
        let mut rules = BudgetRules::default();
        rules.synchronize("first", &[1, 2]);
        rules.configure(1, 1000).unwrap();
        rules.configure(2, 2000).unwrap();
        rules.synchronize("first", &[1]);
        assert_eq!(rules.rules.len(), 1);
        rules.synchronize("second", &[1]);
        assert!(rules.rules.is_empty());
    }
}
