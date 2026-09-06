/* Keep the shared interpreter in instruction RAM. This is board placement,
 * not a second runtime implementation. Match the Rust symbol's stable method
 * suffix, including its literals, independently of the crate disambiguator. */
*(.literal.*2Vm3run .text.*2Vm3run)
*(.literal.*18run_sample_program .text.*18run_sample_program)
*(.literal.*19sample_signal_graph* .text.*19sample_signal_graph*)
*(.literal.*18sample_layer_frame* .text.*18sample_layer_frame*)

/* The Wi-Fi build prepares and services four RMT streams while flash cache is
 * shared with the radio. Keep the small encoder and joined RMT future hot. */
*(.literal.*16esp_hal_smartled*5write* .text.*16esp_hal_smartled*5write*)
*(.literal.*15embassy_futures4join* .text.*15embassy_futures4join*)

/* Profiling experiment: division and its literals are hot in interrupted-PC
 * samples. Keep both together to measure flash-placement cost. */
*(.literal.*8___divsf3 .text.*8___divsf3)
*(.literal.__divsf3 .text.__divsf3)
