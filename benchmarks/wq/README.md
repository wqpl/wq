# wq runtime benchmark programs

These programs are stable, targeted inputs for `wqbench.py`. Each program focuses
on one runtime path and uses a fixed problem size large enough to dominate process
startup under Cargo profile `R`.

Keep benchmark names and workloads unchanged when possible so historical timings
remain comparable. Add a new program when coverage needs to grow. Change an
existing program only when its workload no longer exercises the behavior named by
the file. `wqbench.py` records a source hash and starts a new history series after
an intentional workload change.

Every program validates its result so the benchmark preflight rejects semantic
failures before timing begins.
