# Timberborn PlatformCruncher

Timberborn's 3D terrain, which can be supported using platforms, is an essential part of gameplay. Due to the fairly limited range of supports, and the fact that they take up a spot on the ground, finding an efficient layout can mean slightly more space on the ground for plants, for instance.

This project uses a SAT solver to find optimal support layouts for terrain of a given shape. The program currently only uses basic 1x1 supports.

# Requirements

Besides everything that cargo already handles automatically, `rustsat-glucose` requires `bindgen`, which has [special installation requirements][bindgen-install]. If your build is failing, make sure that you have `libclang` (see link) and CMake installed.

[bindgen-install]: https://rust-lang.github.io/rust-bindgen/requirements.html

# Usage

**TODO:** Rewrite usage guide - the program now provides REPL rather than using CLI args. The REPL is fairly self-describing, and works quite similarly, but the guide below is still out of date.

```
cargo run -r -- <START_COUNT> rect <WIDTH> <HEIGHT>
cargo run -r -- <START_COUNT> file <PATH>
```
_Using `-r` (`--release`) is recommended_

The solver begins at `START_COUNT` supports as the upper limit, and incrementally refines the limit until it finds an unsatisfiable solution (or until it is terminated via Ctrl-C). It's usually a good idea to give it a fairly high initial estimate, since those solutions tend to be trivial to find anyway.

`rect` makes it solve for a simple rectangular area. `file` reads the ceiling pattern from a text file - each line is a row, `X` represents a ceiling block, and ` ` or `.` represent empty space (no ceiling). Trailing spaces are not required.

If you run the program without any arguments, you may enter the arguments via stdin.

Solutions are printed in a graphical representation - `░` for empty space, `▒` for a ceiling block overhead, and `█` for a 1x1 support block.

As the solver approaches an optimal solution, the solving time can begin to grow rapidly. If you don't want to wait for minutes (or even hours) just to save one more space on the ground, you can terminate the solver at any time via Ctrl-C - just make sure that your terminal window doesn't close automatically, or copy the printed solutions beforehand. Stopping the solver can take a moment, but if you believe that the program got stuck, pressing Ctrl-C again will abort immediately.

## Example output

<details>

<summary>Click to show output</summary>

```
cargo run -r -- 30 file test.txt
```

```
Opening file [...]/test.txt
Solving for n <= 30...
Solution: (18 marked)
▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  
▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  
▒  █  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  
▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  
▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ░  ░  ░  ░  
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  
▒  ▒  █  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  
▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  █  ▒  ░  ░  
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░
▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ░
▒  █  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░
▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  █  ▒  ░  ░  ░  ▒  █  ▒  ▒  ▒  █  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  ▒  ▒  ▒  ▒  ▒  ▒  ▒
Solving for n <= 17...
Solution: (17 marked)
▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░
▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░
▒  █  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░
▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░
▒  ▒  █  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░
▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  █  ▒  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░
█  ▒  ▒  ▒  ▒  ▒  █  ▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░
▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  █  ▒  ░  ░  ░  █  ▒  ▒  ▒  ▒  █  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  ▒  ▒  ▒  ▒  ▒  ▒  ▒
Solving for n <= 16...
Solution: (16 marked)
▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░
▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░
▒  █  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░
▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░
▒  ▒  █  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░
▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  █  ▒  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░
█  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░
▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  █  ▒  ░  ░  ░  █  ▒  ▒  ▒  ▒  █  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  ▒  ▒  ▒  ▒  ▒  ▒  ▒
Solving for n <= 15...
Solution: (15 marked)
▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░
▒  █  ▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░  ░  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░
▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ░  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░
▒  ▒  ▒  █  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  █  ▒  ░  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░
█  ▒  ▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░
▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ▒  ░  ░  ░  █  ▒  ▒  ▒  ▒  █  ░
▒  ▒  ▒  ▒  █  ▒  ▒  ▒  ▒  █  ▒  ░  ░  ░  ▒  ▒  ▒  ▒  ▒  ▒  ▒
Solving for n <= 14...
[2025-07-05T04:10:48Z WARN  timberborn_support_solver] Stopping...
Interrupted!

Process finished with exit code 0
```

</details>

# Notes

This program uses the Glucose SAT solver. This should work fine on both Windows and Linux - whereas, for instance, the CaDiCaL crate currently doesn't work on Windows - but should there be any issues with getting the SAT solver to work, you may need to modify the source to use a different backend.
