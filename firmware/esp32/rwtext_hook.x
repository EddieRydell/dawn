/* Keep the shared interpreter in instruction RAM. This is board placement,
 * not a second runtime implementation. Match the Rust symbol's stable method
 * suffix, including its literals, independently of the crate disambiguator. */
*(.literal.*2Vm3run .text.*2Vm3run)
*(.literal.*18run_sample_program .text.*18run_sample_program)
*(.literal.*19sample_signal_graph* .text.*19sample_signal_graph*)
*(.literal.*18sample_layer_frame* .text.*18sample_layer_frame*)

/* Profiling experiment: division and its literals are hot in interrupted-PC
 * samples. Keep both together to measure flash-placement cost. */
*(.literal.*8___divsf3 .text.*8___divsf3)
*(.literal.__divsf3 .text.__divsf3)
