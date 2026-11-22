# Architecture

Follow [AGENTS.md](../AGENTS.md) and any locally supplied task requirements.
Keep the canvas and byte-oriented ANSI, encoding, and SAUCE logic in
`ansi_core`, independent of the `adraw` GUI. Test format and model
changes in the core crate. Report invalid input rather than silently losing it.

