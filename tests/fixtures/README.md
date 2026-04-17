# tests/fixtures/

Real BT wire captures from live inverters, one frame per `.hex` file.

- `captured/` — parsed out of `SBFspot -debug=5` output via
  `scripts/parse-sbfspot-hexdump.py`. Filename convention:
  `NNNN-{send|recv}.hex`.

Each file is whitespace-separated hex bytes, one line per frame (no byte
stuffing removed; the parser sees the raw wire bytes). Example contents:

    7e 1a 00 64 04 42 1a 5a 37 74 ff ff ff ff ff ff 01 00 09 a0 ... 7e
