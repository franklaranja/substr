 A compact collection of overlapping strings.

 [`SubStr`] stores a collection of strings as one big string
 and a vector with the location and length of each string.
 During the build process substrings are identified. For example
 'substring' can also store the strings 'sub', 'ring' etc. Also
 strings are combined to create new substring locations: e.g.
 by placing 'each' after 'with', 'the' can be stored. This can
 result in significant compression.

 # Limitations

 - The resulting collection is immutable
 - Construction is time consuming
 - Stores strings with a maximum length of `u8::MAX` **bytes**
 - Compression dependent on input (might be small)
 - Access is slower compared to a Vec

 # Feature flags

 - `serde`: makes `SubStr` and `Builder` serializable.
