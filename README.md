## Initial Profiling

No other operations: 14 simultaneous sends (~90 Mbps)
With profiling:      12 sends, 137 us, (~76 Mbps), unstable(?)
                     10 sends, 114 us, (~65 Mbps)
                      8 sends, 92 us,  (~52 Mbps)

## TODO

[X]: Add logging implementation
[ ]: Look into https://docs.rs/ndarray/latest/ndarray/index.html or https://www.nalgebra.org/docs/
     for array structuring