#[derive(Debug, Clone)]
pub enum Symbol {
    Struct {
        name: String,
        fields: Vec<String>,
        line: usize,
    },
    Enum {
        name: String,
        variants: Vec<String>,
        line: usize,
    },
    Trait {
        name: String,
        methods: Vec<String>,
        line: usize,
    },
    Function {
        name: String,
        signature: String,
        line: usize,
    },
    Impl {
        type_name: String,
        trait_name: Option<String>,
        methods: Vec<String>,
        line: usize,
    },
    TypeAlias {
        name: String,
        line: usize,
    },
}

impl Symbol {
    pub fn line(&self) -> usize {
        match self {
            Symbol::Struct { line, .. } => *line,
            Symbol::Enum { line, .. } => *line,
            Symbol::Trait { line, .. } => *line,
            Symbol::Function { line, .. } => *line,
            Symbol::Impl { line, .. } => *line,
            Symbol::TypeAlias { line, .. } => *line,
        }
    }
    pub fn render(&self) -> String {
        match self {
            Symbol::Struct { name, fields, line } => {
                if fields.is_empty() {
                    format!("  struct {}  (line {})", name, line)
                } else {
                    format!("  struct {}  {{ {} }}  (line {})", name, fields.join(", "), line)
                }
            }
            Symbol::Enum { name, variants, line } => {
                format!("  enum {}  {{ {} }}  (line {})", name, variants.join(", "), line)
            }
            Symbol::Trait { name, methods, line } => {
                if methods.is_empty() {
                    format!("  trait {}  (line {})", name, line)
                } else {
                    format!("  trait {}  {{ {} }}  (line {})", name, methods.join(", "), line)
                }
            }
            Symbol::Function { name: _, signature, line } => {
                format!("  {}  (line {})", signature, line)
            }
            Symbol::Impl { type_name, trait_name, methods, line } => {
                let header = match trait_name {
                    Some(t) => format!("impl {} for {}", t, type_name),
                    None => format!("impl {}", type_name),
                };
                if methods.is_empty() {
                    format!("  {}  (line {})", header, line)
                } else {
                    format!("  {}  {{ {} }}  (line {})", header, methods.join(", "), line)
                }
            }
            Symbol::TypeAlias { name, line } => {
                format!("  type {}  (line {})", name, line)
            }
        }
    }
}