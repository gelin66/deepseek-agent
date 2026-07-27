# Journal format

Each committed record is `<utf8-byte-length>:<utf8-payload>\n`. Reopen accepts an incomplete final record as an uncommitted tail, but rejects corruption inside a committed prefix.
