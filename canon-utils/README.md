# canon-utils

## Binary TLOG Default

Binary event logs are now the default.

Logs are written to:

```
state/kernel_logs/kernel.tlog.d/
```

### Environment Overrides

- `CANON_TLOG_FORMAT=jsonl` forces legacy JSONL logging.
- `CANON_TLOG_RETAIN_SEGMENTS=10` limits binary segment retention.
- `CANON_TLOG_DUAL_WRITE=1` enables dual logging (binary + JSONL) for verification.

## Replay

Use `reports_from_tlog --tlog <path>` with either:

- `kernel.tlog` (JSONL)
- `kernel.tlog.d/` (binary segments)
