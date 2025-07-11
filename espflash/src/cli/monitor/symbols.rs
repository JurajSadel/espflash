use std::error::Error;

use addr2line::{
    Context,
    LookupResult,
    gimli::{EndianRcSlice, RunTimeEndian},
    object::{Object, ObjectSegment, ObjectSymbol, read::File},
};

// Wrapper around addr2line that allows to look up function names and
// locations from a given address.
pub(crate) struct Symbols<'sym> {
    object: File<'sym, &'sym [u8]>,
    ctx: Context<EndianRcSlice<RunTimeEndian>>,
}

impl<'sym> Symbols<'sym> {
    /// Tries to create a new `Symbols` instance from the given ELF file bytes.
    pub fn try_from(bytes: &'sym [u8]) -> Result<Self, Box<dyn Error>> {
        let object = File::parse(bytes)?;
        let ctx = Context::new(&object)?;

        Ok(Self { object, ctx })
    }

    /// Returns the name of the function at the given address, if one can be
    /// found.
    pub fn name(&self, addr: u64) -> Option<String> {
        // No need to try an address not contained in any segment:
        if !self.object.segments().any(|segment| {
            (segment.address()..(segment.address() + segment.size())).contains(&addr)
        }) {
            return None;
        }

        // The basic steps here are:
        //   1. Find which frame `addr` is in
        //   2. Look up and demangle the function name
        //   3. If no function name is found, try to look it up in the object file
        //      directly
        //   4. Return a demangled function name, if one was found
        let mut frames = match self.ctx.find_frames(addr) {
            LookupResult::Output(result) => result.unwrap(),
            LookupResult::Load { .. } => unimplemented!(),
        };

        frames
            .next()
            .ok()
            .flatten()
            .and_then(|frame| {
                frame
                    .function
                    .and_then(|name| name.demangle().map(|s| s.into_owned()).ok())
            })
            .or_else(|| {
                // Don't use `symbol_map().get(addr)` - it's documentation says "Get the symbol
                // before the given address." which might be totally wrong
                let symbol = self.object.symbols().find(|symbol| {
                    (symbol.address()..=(symbol.address() + symbol.size())).contains(&addr)
                });

                if let Some(symbol) = symbol {
                    match symbol.name() {
                        Ok(name) if !name.is_empty() => Some(
                            addr2line::demangle_auto(std::borrow::Cow::Borrowed(name), None)
                                .to_string(),
                        ),
                        _ => None,
                    }
                } else {
                    None
                }
            })
    }

    /// Returns the file name and line number of the function at the given
    /// address, if one can be.
    pub fn location(&self, addr: u64) -> Option<(String, u32)> {
        // Find the location which `addr` is in. If we can dedetermine a file name and
        // line number for this function we will return them both in a tuple.
        self.ctx.find_location(addr).ok()?.map(|location| {
            let file = location.file.map(|f| f.to_string());
            let line = location.line;

            match (file, line) {
                (Some(file), Some(line)) => Some((file, line)),
                _ => None,
            }
        })?
    }
}
