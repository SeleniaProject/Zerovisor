use zerovisor_hal::interrupts::{record_latency_ns, worst_case_latency_ns, MAX_INTERRUPT_LATENCY_NS};

// Simulated measurements – in real hardware path these would be called by ISR wrapper.
#[test]
fn worst_case_interrupt_latency_within_target() {
    // Feed synthetic latencies below threshold.
    for ns in [100u64, 300, 700, 950] {
        record_latency_ns(ns);
    }
    // Feed one latency exactly at threshold.
    record_latency_ns(MAX_INTERRUPT_LATENCY_NS);
    assert!(worst_case_latency_ns() <= MAX_INTERRUPT_LATENCY_NS);
} 