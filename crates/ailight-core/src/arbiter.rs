//! 状态仲裁器（ADR-0001 Q8，KAD-13）
//!
//! 规则：
//! - 最近活跃：任何非幂等事件都生效，最后上报的工具接管灯效
//! - 终态驻留（hold_ms）：进入 SUCCESS/ERROR 且配置了 hold_ms>0 时，到期自动回落 IDLE

use serde::Serialize;

/// 标准状态名（ADR-0001 Q1）
pub const ST_IDLE: &str = "IDLE";
pub const ST_WORKING: &str = "WORKING";
pub const ST_WAITING: &str = "WAITING";
pub const ST_SUCCESS: &str = "SUCCESS";
pub const ST_ERROR: &str = "ERROR";

/// 标准状态优先级（数值越大越优先）
pub fn state_priority(state: &str) -> u8 {
    match state {
        ST_ERROR => 5,
        ST_SUCCESS => 4,
        ST_WORKING => 3,
        ST_WAITING => 2,
        ST_IDLE => 0,
        // 自定义状态：IDLE 之上、WAITING 之下
        _ => 1,
    }
}

/// 是否标准状态
pub fn is_standard_state(state: &str) -> bool {
    matches!(
        state,
        ST_IDLE | ST_WORKING | ST_WAITING | ST_SUCCESS | ST_ERROR
    )
}

/// 一次 hook 事件（hook-api V1.0 的 state_change 归一化）
#[derive(Debug, Clone, PartialEq)]
pub struct HookEvent {
    pub source: String,
    pub state: String,
    pub session: Option<String>,
    pub ts_ms: u64,
}

/// 当前业务状态（唯一事实源，KAD-03）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BusinessState {
    pub state: String,
    pub source: Option<String>,
    pub session: Option<String>,
    pub since_ms: u64,
    /// 终态驻留截止时间；None = 不驻留（驻留到下一事件或永久）
    pub hold_until_ms: Option<u64>,
}

impl BusinessState {
    pub fn idle(now_ms: u64) -> Self {
        Self {
            state: ST_IDLE.to_string(),
            source: None,
            session: None,
            since_ms: now_ms,
            hold_until_ms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApplyOutcome {
    /// 状态已变更（applied=true）
    Applied(BusinessState),
    /// 状态未变更（applied=false，幂等）
    NoChange(BusinessState),
}

#[derive(Debug)]
pub struct Arbiter {
    current: BusinessState,
    /// 事件计数器（最近活跃排序辅助/调试）
    event_count: u64,
}

impl Arbiter {
    pub fn new(now_ms: u64) -> Self {
        Self {
            current: BusinessState::idle(now_ms),
            event_count: 0,
        }
    }

    pub fn current(&self) -> &BusinessState {
        &self.current
    }

    /// 处理 hook 事件。
    ///
    /// `hold_ms`：本次事件进入终态时的驻留时长（0 = 驻留到下一事件；None = 按主题默认 0）。
    /// 返回是否生效（applied 对账，hook-api §3.2）。
    pub fn apply(&mut self, ev: &HookEvent, hold_ms: Option<u64>, now_ms: u64) -> ApplyOutcome {
        self.event_count += 1;
        let hold = hold_ms.unwrap_or(0);
        let hold_until = if hold > 0 { Some(now_ms + hold) } else { None };

        // 相同 source + state 且未驻留中 → 幂等不重复（hook-api §3.2）
        if self.current.state == ev.state
            && self.current.source.as_deref() == Some(ev.source.as_str())
        {
            return ApplyOutcome::NoChange(self.current.clone());
        }

        self.current = BusinessState {
            state: ev.state.clone(),
            source: Some(ev.source.clone()),
            session: ev.session.clone(),
            since_ms: now_ms,
            hold_until_ms: hold_until,
        };
        ApplyOutcome::Applied(self.current.clone())
    }

    /// 驻留到期回落检查：hold_until 到期 → 回落 IDLE
    pub fn tick(&mut self, now_ms: u64) -> Option<BusinessState> {
        if let Some(until) = self.current.hold_until_ms {
            if now_ms >= until {
                self.current = BusinessState::idle(now_ms);
                return Some(self.current.clone());
            }
        }
        None
    }

    /// 手动复位（reset_outputs 联动，ipc-contract §2.4）
    pub fn reset(&mut self, now_ms: u64) -> BusinessState {
        self.current = BusinessState::idle(now_ms);
        self.current.clone()
    }

    /// 事件计数（调试）
    pub fn event_count(&self) -> u64 {
        self.event_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(source: &str, state: &str, ts: u64) -> HookEvent {
        HookEvent {
            source: source.to_string(),
            state: state.to_string(),
            session: None,
            ts_ms: ts,
        }
    }

    fn applied(out: &ApplyOutcome) -> bool {
        matches!(out, ApplyOutcome::Applied(_))
    }

    #[test]
    fn idempotent_same_source_state() {
        let mut a = Arbiter::new(0);
        assert!(applied(&a.apply(&ev("cc", ST_WORKING, 1), None, 1)));
        // 相同 source+state 重复 → NoChange（applied=false）
        assert!(!applied(&a.apply(&ev("cc", ST_WORKING, 2), None, 2)));
        // 不同 source 相同 state → 覆盖（最近活跃）
        assert!(applied(&a.apply(&ev("codex", ST_WORKING, 3), None, 3)));
    }

    #[test]
    fn latest_source_always_takes_over() {
        let mut a = Arbiter::new(0);
        assert!(applied(&a.apply(&ev("claude-code", ST_ERROR, 1), None, 1)));
        assert!(applied(&a.apply(&ev("codex", ST_WAITING, 2), None, 2)));
        assert_eq!(a.current().source.as_deref(), Some("codex"));
        assert_eq!(a.current().state, ST_WAITING);
        assert!(applied(&a.apply(
            &ev("claude-code", ST_WORKING, 3),
            None,
            3
        )));
        assert_eq!(a.current().source.as_deref(), Some("claude-code"));
        assert_eq!(a.current().state, ST_WORKING);
    }

    #[test]
    fn hold_rollback_to_idle() {
        let mut a = Arbiter::new(0);
        // SUCCESS hold 5000ms
        assert!(applied(&a.apply(
            &ev("cc", ST_SUCCESS, 1000),
            Some(5000),
            1000
        )));
        assert_eq!(a.current().hold_until_ms, Some(6000));
        // 到期前 tick 无变化
        assert!(a.tick(5999).is_none());
        // 到期 tick 回落 IDLE
        let changed = a.tick(6000).unwrap();
        assert_eq!(changed.state, ST_IDLE);
    }

    #[test]
    fn hold_zero_means_stay_until_next_event() {
        let mut a = Arbiter::new(0);
        // ERROR hold 0 → 驻留到下一事件（无 hold_until）
        assert!(applied(&a.apply(&ev("cc", ST_ERROR, 1), Some(0), 1)));
        assert_eq!(a.current().hold_until_ms, None);
        // tick 不会回落
        assert!(a.tick(999_999).is_none());
        // 下一事件（IDLE）清除
        assert!(applied(&a.apply(&ev("cc", ST_IDLE, 2), None, 2)));
    }

    #[test]
    fn custom_state_is_supported() {
        assert_eq!(state_priority("REVIEW"), 1);
        let mut a = Arbiter::new(0);
        assert!(applied(&a.apply(&ev("cc", "REVIEW", 1), None, 1)));
        assert!(applied(&a.apply(&ev("cc", ST_WAITING, 2), None, 2)));
        assert!(applied(&a.apply(&ev("codex", "REVIEW", 3), None, 3)));
    }

    #[test]
    fn reset_clears() {
        let mut a = Arbiter::new(0);
        a.apply(&ev("cc", ST_ERROR, 1), None, 1);
        let s = a.reset(100);
        assert_eq!(s.state, ST_IDLE);
        assert_eq!(a.current().state, ST_IDLE);
    }
}
