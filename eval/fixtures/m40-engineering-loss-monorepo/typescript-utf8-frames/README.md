# UTF-8 frame decoder

Frames are newline-delimited JSON objects with a non-negative integer `sequence` field. Input chunks may split any UTF-8 code point or line ending.
