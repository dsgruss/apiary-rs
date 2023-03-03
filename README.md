Introduction
------------

This project is part of an experiment in creating a
[polyphonic](https://en.wikipedia.org/wiki/Polyphony) version of a [modular
synthesizer](https://en.wikipedia.org/wiki/Modular_synthesizer) using ethernet communication. The
high-level goal is to determine if maintaining low-latency (~1 ms per module) high-throughput (8
parallel channels of uncompressed audio) communication is possible on consumer-level networking
hardware and low-cost microcontrollers.

`apiary` is divided into two packages: `core` contains the shared communication protocol for control
and audio transfer, different networking backend implementations, and shared DSP algorithms. The
`examples` directory within `core` contains an [egui](https://github.com/emilk/egui)-based staging
ground for developing and testing module concepts on a host machine. The `stm32` package has
specific implementations of modules and drivers for use with embedded development.
