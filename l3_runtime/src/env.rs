use std::io::{BufRead, BufReader, LineWriter, Read, Write};

use rand::{SeedableRng as _, rngs::StdRng};

/// The VM's process environment: buffered stdio and the seeded RNG. Kept
/// separate from the GC heap so memory management and I/O are not fused.
pub struct RuntimeEnv<'a> {
    pub input: Box<dyn BufRead + 'a>,
    pub output: Box<dyn Write + 'a>,
    pub rng: StdRng,
}

impl<'a> RuntimeEnv<'a> {
    #[must_use]
    pub fn new(writer: &'a mut impl Write, reader: &'a mut impl Read) -> Self {
        Self {
            input: Box::new(BufReader::new(reader)),
            output: Box::new(LineWriter::new(writer)),
            rng: StdRng::seed_from_u64(42),
        }
    }
}
