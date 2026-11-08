//! An exported function's signature as its compiler describes it in debug info (DWARF), compared
//! at the level the C calling convention sees: how many arguments, and for each (and the return)
//! whether it is an integer, a float, a pointer or an aggregate, of what size and signedness.
//!
//! A linker matches symbols by name only, so a Rust export whose parameters disagree with the C
//! declaration links cleanly and corrupts the program at run time. Both sides are compiled with
//! `-g` and read here, so the comparison uses what each compiler actually decided (typedefs,
//! `enum` widths, `#[repr(C)]` sizes) rather than a reading of the source text.

use std::borrow::Cow;

use gimli::{AttributeValue, DwTag, RunTimeEndian, UnitOffset};
use object::{Object, ObjectSection};

/// How a value is passed, as far as this check can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Class {
    Void,
    /// `signed` is `None` when it isn't known (an `enum`).
    Int {
        size: u64,
        signed: Option<bool>,
    },
    Float {
        size: u64,
    },
    Pointer,
    /// A struct or union passed by value; compared by size only.
    Aggregate {
        size: u64,
    },
    /// Not classified (arrays, incomplete types, references across units): never a mismatch.
    Unknown,
}

impl Class {
    fn describe(&self) -> String {
        match self {
            Class::Void => "nothing".into(),
            Class::Int {
                size,
                signed: Some(true),
            } => format!("{size}-byte signed integer"),
            Class::Int {
                size,
                signed: Some(false),
            } => format!("{size}-byte unsigned integer"),
            Class::Int { size, signed: None } => format!("{size}-byte integer"),
            Class::Float { size } => format!("{size}-byte float"),
            Class::Pointer => "pointer".into(),
            Class::Aggregate { size } => format!("{size}-byte struct or union, by value"),
            Class::Unknown => "unclassified".into(),
        }
    }

    /// Whether passing `self` where `other` is expected is wrong.
    fn differs(&self, other: &Class) -> bool {
        use Class::*;
        match (self, other) {
            (Unknown, _) | (_, Unknown) => false,
            (Int { size: a, signed: s }, Int { size: b, signed: t }) => {
                a != b || matches!((s, t), (Some(s), Some(t)) if s != t)
            }
            (Float { size: a }, Float { size: b })
            | (Aggregate { size: a }, Aggregate { size: b }) => a != b,
            (Pointer, Pointer) | (Void, Void) => false,
            _ => true,
        }
    }
}

/// A parameter or return value: its class, and its type as the compiler spelled it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub class: Class,
    pub spelled: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// Each parameter's name, if the debug info has one, and value.
    pub params: Vec<(Option<String>, Value)>,
    pub ret: Value,
    pub variadic: bool,
}

impl Signature {
    /// One line, for logs: `(a: int, b: char *) -> int`.
    pub fn show(&self) -> String {
        let mut params: Vec<String> = self
            .params
            .iter()
            .map(|(name, v)| match name {
                Some(n) => format!("{n}: {}", v.spelled),
                None => v.spelled.clone(),
            })
            .collect();
        if self.variadic {
            params.push("...".into());
        }
        format!("({}) -> {}", params.join(", "), self.ret.spelled)
    }
}

/// Why the Rust export `rust` can't stand in for the C function `c`: one line per difference,
/// empty when they agree.
pub fn compare(c: &Signature, rust: &Signature) -> Vec<String> {
    let mut out = Vec::new();
    if c.variadic {
        out.push(
            "the C function is variadic (`...`): stable Rust cannot define a variadic \
             `extern \"C\"` function, so this chunk cannot be replaced as it is"
                .into(),
        );
    }
    if c.params.len() != rust.params.len() {
        out.push(format!(
            "C takes {} parameter(s), the Rust export takes {}",
            c.params.len(),
            rust.params.len()
        ));
    } else {
        for (i, ((name, cv), (_, rv))) in c.params.iter().zip(&rust.params).enumerate() {
            if cv.class.differs(&rv.class) {
                let name = name.as_deref().map_or(String::new(), |n| format!(" `{n}`"));
                out.push(format!(
                    "parameter {}{name}: C passes `{}` ({}), the Rust export takes `{}` ({})",
                    i + 1,
                    cv.spelled,
                    cv.class.describe(),
                    rv.spelled,
                    rv.class.describe()
                ));
            }
        }
    }
    if c.ret.class.differs(&rust.ret.class) {
        out.push(format!(
            "return value: C returns `{}` ({}), the Rust export returns `{}` ({})",
            c.ret.spelled,
            c.ret.class.describe(),
            rust.ret.spelled,
            rust.ret.class.describe()
        ));
    }
    out
}

/// The signature of the function whose symbol is `symbol` in the object file `bytes`, from its
/// debug info; `None` if it has none. A definition is preferred to a declaration.
pub fn signature(bytes: &[u8], symbol: &str) -> Result<Option<Signature>, String> {
    let file = object::File::parse(bytes).map_err(|e| format!("reading the object: {e}"))?;
    let endian = if file.is_little_endian() {
        RunTimeEndian::Little
    } else {
        RunTimeEndian::Big
    };
    // An unlinked object's debug info points into its string and type sections through
    // relocations, so they must be applied while reading.
    let load = |id: gimli::SectionId| -> Result<Section<'_>, object::Error> {
        Ok(match file.section_by_name(id.name()) {
            Some(s) => Section {
                data: s.uncompressed_data()?,
                relocations: Relocations(s.relocation_map()?),
            },
            None => Section {
                data: Cow::Borrowed(&[]),
                relocations: Relocations::default(),
            },
        })
    };
    let sections =
        gimli::DwarfSections::load(load).map_err(|e| format!("reading the object: {e}"))?;
    let dwarf = sections.borrow(|s| {
        gimli::RelocateReader::new(gimli::EndianSlice::new(&s.data, endian), &s.relocations)
    });
    find(&dwarf, symbol).map_err(|e| format!("reading debug info: {e}"))
}

struct Section<'d> {
    data: Cow<'d, [u8]>,
    relocations: Relocations,
}

#[derive(Debug, Default)]
struct Relocations(object::read::RelocationMap);

impl gimli::read::Relocate for &Relocations {
    fn relocate_address(&self, offset: usize, value: u64) -> gimli::Result<u64> {
        Ok(self.0.relocate(offset as u64, value))
    }

    fn relocate_offset(&self, offset: usize, value: usize) -> gimli::Result<usize> {
        <usize as gimli::ReaderOffset>::from_u64(self.0.relocate(offset as u64, value as u64))
    }
}

type R<'a> = gimli::RelocateReader<gimli::EndianSlice<'a, RunTimeEndian>, &'a Relocations>;
type Dwarf<'a> = gimli::Dwarf<R<'a>>;
type Unit<'a> = gimli::Unit<R<'a>>;
type Die<'a> = gimli::DebuggingInformationEntry<R<'a>>;

/// Pointer chains and typedefs deeper than this are left unclassified.
const DEPTH: u32 = 16;

fn find(dwarf: &Dwarf<'_>, symbol: &str) -> gimli::Result<Option<Signature>> {
    let mut declared = None;
    let mut units = dwarf.units();
    while let Some(header) = units.next()? {
        let unit = dwarf.unit(header)?;
        let mut found = Vec::new();
        let mut cursor = unit.entries();
        while let Some(die) = cursor.next_dfs()? {
            if die.tag() != gimli::DW_TAG_subprogram {
                continue;
            }
            // A `#[no_mangle]`/C function has no linkage name, or one equal to its symbol; a
            // private Rust function of the same name has a mangled one.
            let linkage = string(dwarf, &unit, die, gimli::DW_AT_linkage_name)?;
            let name = string(dwarf, &unit, die, gimli::DW_AT_name)?;
            let ours = match linkage.as_deref() {
                Some(l) => l == symbol,
                None => name.as_deref() == Some(symbol),
            };
            if ours {
                found.push((die.offset(), die.has_attr(gimli::DW_AT_declaration)));
            }
        }
        for (offset, declaration) in found {
            let signature = describe(dwarf, &unit, offset)?;
            if !declaration {
                return Ok(Some(signature));
            }
            declared.get_or_insert(signature);
        }
    }
    Ok(declared)
}

fn describe(dwarf: &Dwarf<'_>, unit: &Unit<'_>, at: UnitOffset) -> gimli::Result<Signature> {
    let mut tree = unit.entries_tree(Some(at))?;
    let root = tree.root()?;
    let ret = value(dwarf, unit, root.entry().attr_value(gimli::DW_AT_type), 0)?;
    let (mut params, mut variadic) = (Vec::new(), false);
    let mut children = root.children();
    while let Some(child) = children.next()? {
        let die = child.entry();
        match die.tag() {
            gimli::DW_TAG_formal_parameter if !die.has_attr(gimli::DW_AT_artificial) => {
                let name = string(dwarf, unit, die, gimli::DW_AT_name)?;
                params.push((
                    name,
                    value(dwarf, unit, die.attr_value(gimli::DW_AT_type), 0)?,
                ));
            }
            gimli::DW_TAG_unspecified_parameters => variadic = true,
            _ => {}
        }
    }
    Ok(Signature {
        params,
        ret,
        variadic,
    })
}

fn string(
    dwarf: &Dwarf<'_>,
    unit: &Unit<'_>,
    die: &Die<'_>,
    at: gimli::DwAt,
) -> gimli::Result<Option<String>> {
    use gimli::Reader;
    match die.attr_value(at) {
        Some(v) => Ok(Some(
            dwarf.attr_string(unit, v)?.to_string_lossy()?.into_owned(),
        )),
        None => Ok(None),
    }
}

fn unknown(spelled: impl Into<String>) -> Value {
    Value {
        class: Class::Unknown,
        spelled: spelled.into(),
    }
}

/// The value of type `ty` (a `DW_AT_type`; none is `void`).
fn value(
    dwarf: &Dwarf<'_>,
    unit: &Unit<'_>,
    ty: Option<AttributeValue<R<'_>>>,
    depth: u32,
) -> gimli::Result<Value> {
    let offset = match ty {
        None => {
            return Ok(Value {
                class: Class::Void,
                spelled: "void".into(),
            });
        }
        Some(AttributeValue::UnitRef(offset)) => offset,
        Some(_) => return Ok(unknown("<type in another unit>")),
    };
    if depth > DEPTH {
        return Ok(unknown("<deeply nested type>"));
    }
    let die = unit.entry(offset)?;
    let name = string(dwarf, unit, &die, gimli::DW_AT_name)?;
    let size = die
        .attr_value(gimli::DW_AT_byte_size)
        .and_then(|v| v.udata_value());
    let inner = die.attr_value(gimli::DW_AT_type);
    let tag = die.tag();
    let value = match tag {
        gimli::DW_TAG_typedef
        | gimli::DW_TAG_const_type
        | gimli::DW_TAG_volatile_type
        | gimli::DW_TAG_restrict_type
        | gimli::DW_TAG_atomic_type => {
            let v = value(dwarf, unit, inner, depth + 1)?;
            let spelled = match (tag, name) {
                (gimli::DW_TAG_typedef, Some(n)) => n,
                (gimli::DW_TAG_const_type, _) => format!("const {}", v.spelled),
                (gimli::DW_TAG_volatile_type, _) => format!("volatile {}", v.spelled),
                _ => v.spelled.clone(),
            };
            Value {
                class: v.class,
                spelled,
            }
        }
        gimli::DW_TAG_base_type => {
            let size = size.unwrap_or(0);
            let encoding = match die.attr_value(gimli::DW_AT_encoding) {
                Some(AttributeValue::Encoding(e)) => Some(e),
                _ => None,
            };
            let class = match encoding {
                _ if size == 0 => Class::Void,
                Some(gimli::DW_ATE_float) => Class::Float { size },
                Some(gimli::DW_ATE_signed | gimli::DW_ATE_signed_char) => Class::Int {
                    size,
                    signed: Some(true),
                },
                Some(
                    gimli::DW_ATE_unsigned
                    | gimli::DW_ATE_unsigned_char
                    | gimli::DW_ATE_boolean
                    | gimli::DW_ATE_UTF,
                ) => Class::Int {
                    size,
                    signed: Some(false),
                },
                _ => Class::Unknown,
            };
            Value {
                class,
                spelled: name.unwrap_or_else(|| "<base type>".into()),
            }
        }
        gimli::DW_TAG_pointer_type
        | gimli::DW_TAG_reference_type
        | gimli::DW_TAG_rvalue_reference_type => {
            let spelled = match name {
                Some(n) => n,
                None => format!("{} *", pointee(dwarf, unit, inner, depth)?),
            };
            Value {
                class: Class::Pointer,
                spelled,
            }
        }
        gimli::DW_TAG_structure_type | gimli::DW_TAG_union_type | gimli::DW_TAG_class_type => {
            let spelled = name.unwrap_or_else(|| "<anonymous struct>".into());
            // Rust's niche-optimized wrappers are passed exactly as the pointer they hold.
            let pointer_like = [
                "Option<",
                "NonNull<",
                "core::option::Option<",
                "core::ptr::",
            ]
            .iter()
            .any(|p| spelled.starts_with(p));
            let class = match size {
                Some(8) if pointer_like => Class::Pointer,
                _ if die.has_attr(gimli::DW_AT_declaration) => Class::Unknown,
                Some(size) => Class::Aggregate { size },
                None => Class::Unknown,
            };
            Value { class, spelled }
        }
        gimli::DW_TAG_enumeration_type => Value {
            class: Class::Int {
                size: size.unwrap_or(4),
                signed: None,
            },
            spelled: name.unwrap_or_else(|| "<anonymous enum>".into()),
        },
        gimli::DW_TAG_array_type => unknown("<array>"),
        gimli::DW_TAG_subroutine_type => unknown("<function>"),
        other => unknown(format!("<{}>", tag_name(other))),
    };
    Ok(value)
}

/// How a pointer's target is spelled, without classifying it (it may be recursive).
fn pointee(
    dwarf: &Dwarf<'_>,
    unit: &Unit<'_>,
    ty: Option<AttributeValue<R<'_>>>,
    depth: u32,
) -> gimli::Result<String> {
    match ty {
        None => Ok("void".into()),
        Some(AttributeValue::UnitRef(offset)) if depth < DEPTH => {
            let die = unit.entry(offset)?;
            if let Some(n) = string(dwarf, unit, &die, gimli::DW_AT_name)? {
                let kind = match die.tag() {
                    gimli::DW_TAG_structure_type => "struct ",
                    gimli::DW_TAG_union_type => "union ",
                    gimli::DW_TAG_enumeration_type => "enum ",
                    _ => "",
                };
                return Ok(format!("{kind}{n}"));
            }
            match die.tag() {
                gimli::DW_TAG_pointer_type => Ok(format!(
                    "{} *",
                    pointee(dwarf, unit, die.attr_value(gimli::DW_AT_type), depth + 1)?
                )),
                gimli::DW_TAG_const_type => Ok(format!(
                    "const {}",
                    pointee(dwarf, unit, die.attr_value(gimli::DW_AT_type), depth + 1)?
                )),
                gimli::DW_TAG_subroutine_type => Ok("<function>".into()),
                _ => Ok(value(
                    dwarf,
                    unit,
                    Some(AttributeValue::UnitRef(offset)),
                    depth + 1,
                )?
                .spelled),
            }
        }
        Some(_) => Ok("?".into()),
    }
}

fn tag_name(tag: DwTag) -> String {
    tag.static_string()
        .map_or_else(|| format!("tag {:#x}", tag.0), str::to_string)
}

#[cfg(test)]
mod tests;
