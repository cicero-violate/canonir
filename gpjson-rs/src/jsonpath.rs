use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    End = 0,
    StoreResult = 1,
    MoveUp = 2,
    MoveDown = 3,
    MoveToKey = 4,
    MoveToIndex = 5,
    ExpressionStringEquals = 6,
}

#[derive(Debug)]
pub enum JSONPathError {
    Message(String),
    Unsupported(String),
}

impl fmt::Display for JSONPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JSONPathError::Message(msg) => write!(f, "{msg}"),
            JSONPathError::Unsupported(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for JSONPathError {}

pub struct IRByteOutputBuffer {
    buffer: Vec<u8>,
}

impl IRByteOutputBuffer {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn write_opcode(&mut self, opcode: Opcode) {
        self.buffer.push(opcode as u8);
    }

    pub fn write_byte(&mut self, b: u8) {
        self.buffer.push(b);
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    pub fn write_varint(&mut self, mut value: u32) {
        while (value & 0xFFFF_FF80) != 0 {
            self.write_byte(((value & 0x7F) as u8) | 0x80);
            value >>= 7;
        }
        self.write_byte((value & 0x7F) as u8);
    }

    pub fn write_string(&mut self, s: &str) {
        let bytes = s.as_bytes();
        self.write_varint(bytes.len() as u32);
        self.write_bytes(bytes);
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.buffer.clone()
    }
}

pub struct IRBuilder {
    buffer: IRByteOutputBuffer,
    current_level: i32,
    num_result_stores: i32,
    ended: bool,
}

impl IRBuilder {
    pub fn new() -> Self {
        Self {
            buffer: IRByteOutputBuffer::new(),
            current_level: 0,
            num_result_stores: 0,
            ended: false,
        }
    }

    pub fn property(&mut self, name: &str) {
        self.buffer.write_opcode(Opcode::MoveToKey);
        self.buffer.write_string(name);
    }

    pub fn index(&mut self, index: i32) {
        self.buffer.write_opcode(Opcode::MoveToIndex);
        self.buffer.write_varint(index as u32);
    }

    pub fn down(&mut self) {
        self.buffer.write_opcode(Opcode::MoveDown);
        self.current_level += 1;
    }

    pub fn up(&mut self) {
        self.buffer.write_opcode(Opcode::MoveUp);
        self.current_level -= 1;
    }

    pub fn store_result(&mut self) {
        self.buffer.write_opcode(Opcode::StoreResult);
        self.num_result_stores += 1;
    }

    pub fn end(&mut self) {
        if self.ended {
            panic!("IR has already been ended");
        }
        self.ended = true;
        self.buffer.write_opcode(Opcode::End);
    }

    pub fn expression_string_equals(&mut self, s: &str) {
        self.buffer.write_opcode(Opcode::ExpressionStringEquals);
        let mut quoted = String::new();
        quoted.push('"');
        quoted.push_str(s);
        quoted.push('"');
        self.buffer.write_string(&quoted);
    }

    pub fn current_level(&self) -> i32 {
        self.current_level
    }

    pub fn num_result_stores(&self) -> i32 {
        self.num_result_stores
    }

    pub fn into_result(mut self) -> JSONPathResult {
        if !self.ended {
            panic!("Cannot convert to byte array until end() has been called");
        }
        JSONPathResult {
            ir: self.buffer.to_vec(),
            max_depth: 0,
            num_results: self.num_result_stores as usize,
        }
    }

    pub fn buffer(&self) -> &IRByteOutputBuffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut IRByteOutputBuffer {
        &mut self.buffer
    }
}

#[derive(Debug, Clone)]
pub struct JSONPathResult {
    pub ir: Vec<u8>,
    pub max_depth: usize,
    pub num_results: usize,
}

pub struct JSONPathScanner {
    bytes: Vec<u8>,
    position: isize,
    marks: Vec<isize>,
}

impl JSONPathScanner {
    pub fn new(s: &str) -> Self {
        Self {
            bytes: s.as_bytes().to_vec(),
            position: -1,
            marks: Vec::new(),
        }
    }

    pub fn has_next(&self) -> bool {
        (self.position + 1) < self.bytes.len() as isize
    }

    pub fn next(&mut self) -> Result<u8, JSONPathError> {
        if !self.has_next() {
            return Err(JSONPathError::Message(format!(
                "Expected character, got EOF at {}",
                self.position
            )));
        }
        self.position += 1;
        Ok(self.bytes[self.position as usize])
    }

    pub fn peek(&self) -> Result<u8, JSONPathError> {
        if !self.has_next() {
            return Err(JSONPathError::Message(format!(
                "Expected character, got EOF at {}",
                self.position
            )));
        }
        Ok(self.bytes[(self.position + 1) as usize])
    }

    pub fn position(&self) -> isize {
        self.position
    }

    pub fn substring(&self, start: isize, end: isize) -> String {
        let start = (start + 1) as usize;
        let end = (end + 1) as usize;
        String::from_utf8_lossy(&self.bytes[start..end]).to_string()
    }

    pub fn expect_char(&mut self, c: u8) -> Result<(), JSONPathError> {
        let next = self.next()?;
        if next != c {
            return Err(JSONPathError::Message(format!(
                "Expected character '{}', got character '{}' at {}",
                c as char, next as char, self.position
            )));
        }
        Ok(())
    }

    pub fn skip_if_char(&mut self, c: u8) -> Result<bool, JSONPathError> {
        if !self.has_next() {
            return Ok(false);
        }
        let next = self.peek()?;
        if next == c {
            self.position += 1;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn error(&self, msg: &str) -> JSONPathError {
        let current = self.bytes[self.position as usize] as char;
        JSONPathError::Message(format!("{} at {} ('{}')", msg, self.position, current))
    }

    pub fn error_next(&self, msg: &str) -> JSONPathError {
        let current = self.bytes[(self.position + 1) as usize] as char;
        JSONPathError::Message(format!("{} at {} ('{}')", msg, self.position + 1, current))
    }

    pub fn unsupported_next(&self, msg: &str) -> JSONPathError {
        let current = self.bytes[(self.position + 1) as usize] as char;
        JSONPathError::Unsupported(format!("{} at {} ('{}')", msg, self.position + 1, current))
    }

    pub fn mark(&mut self) {
        self.marks.push(self.position);
    }

    pub fn reset(&mut self) {
        if let Some(pos) = self.marks.pop() {
            self.position = pos;
        }
    }
}

pub struct JSONPathParser {
    scanner: JSONPathScanner,
    ir: IRBuilder,
    max_level: usize,
    skip_terminal_store: bool,
}

impl JSONPathParser {
    pub fn new(s: &str) -> Self {
        Self {
            scanner: JSONPathScanner::new(s),
            ir: IRBuilder::new(),
            max_level: 0,
            skip_terminal_store: false,
        }
    }

    pub fn compile(mut self) -> Result<JSONPathResult, JSONPathError> {
        self.scanner.expect_char(b'$')?;
        self.compile_next_expression()?;
        if !self.skip_terminal_store {
            self.ir.store_result();
        }
        self.ir.end();

        let result = JSONPathResult {
            ir: self.ir.buffer().to_vec(),
            max_depth: self.max_level,
            num_results: self.ir.num_result_stores() as usize,
        };

        Ok(result)
    }

    fn compile_next_expression(&mut self) -> Result<(), JSONPathError> {
        match self.scanner.peek()? {
            b'.' => self.compile_dot_expression(),
            b'[' => self.compile_index_expression(),
            _ => Err(self.scanner.unsupported_next("Unsupported expression type")),
        }
    }

    fn compile_dot_expression(&mut self) -> Result<(), JSONPathError> {
        self.scanner.expect_char(b'.')?;
        if self.scanner.peek()? == b'.' {
            return Err(self.scanner.unsupported_next("Unsupported recursive descent"));
        }
        let property = self.read_property()?;
        if property.is_empty() {
            return Err(self.scanner.error("Unexpected empty property"));
        }
        self.create_property_ir(&property);
        if self.scanner.has_next() {
            self.compile_next_expression()?;
        }
        Ok(())
    }

    fn compile_index_expression(&mut self) -> Result<(), JSONPathError> {
        self.scanner.expect_char(b'[')?;
        let next = self.scanner.peek()?;
        if next == b'\'' || next == b'"' {
            let property = self.read_quoted_string()?;
            if property.is_empty() {
                return Err(self.scanner.error("Unexpected empty property"));
            }
            self.create_property_ir(&property);
        } else if (b'0'..=b'9').contains(&next) {
            let index = self.read_integer(|c| c == b']' || c == b':' || c == b',')?;
            match self.scanner.peek()? {
                b':' => {
                    self.compile_index_range_expression(index)?;
                    return Ok(());
                }
                b',' => return Err(self.scanner.unsupported_next("Unsupported multiple index expression")),
                b']' => self.create_index_ir(index),
                _ => return Err(self.scanner.error_next("Unexpected character in index")),
            }
        } else if next == b'*' {
            return Err(self.scanner.unsupported_next("Unsupported wildcard expression"));
        } else if next == b'?' {
            self.compile_filter_expression()?;
        } else {
            return Err(self.scanner.error_next(
                "Unexpected character in index, expected ', \"', or an integer",
            ));
        }

        self.scanner.expect_char(b']')?;

        if self.scanner.has_next() {
            self.compile_next_expression()?;
        }

        Ok(())
    }

    fn compile_index_range_expression(&mut self, start_index: i32) -> Result<(), JSONPathError> {
        self.scanner.expect_char(b':')?;
        let end_index = self.read_integer(|c| c == b']')?;
        self.scanner.expect_char(b']')?;

        let mut max_max_level = self.max_level;

        for i in start_index..end_index {
            let start_level = self.ir.current_level();
            self.ir.index(i);
            self.ir.down();

            self.scanner.mark();

            let current_max_level = self.max_level;

            if self.scanner.has_next() {
                self.compile_next_expression()?;
            }

            max_max_level = max_max_level.max(self.max_level);
            self.max_level = current_max_level;

            // Always store results at the deepest level before moving up.
            self.ir.store_result();

            self.scanner.reset();

            let end_level = self.ir.current_level();
            let diff = end_level - start_level;
            for _ in 0..diff {
                self.ir.up();
            }

            if i == end_index - 1 {
                self.skip_terminal_store = true;
            }
        }

        self.max_level = max_max_level + 1;
        Ok(())
    }

    fn compile_filter_expression(&mut self) -> Result<(), JSONPathError> {
        self.scanner.expect_char(b'?')?;
        self.scanner.expect_char(b'(')?;
        self.scanner.expect_char(b'@')?;

        while self.scanner.skip_if_char(b' ')? {}

        match self.scanner.peek()? {
            b'=' => {
                self.scanner.expect_char(b'=')?;
                self.scanner.expect_char(b'=')?;
                while self.scanner.skip_if_char(b' ')? {}
                let equal_to = self.read_quoted_string()?;
                self.ir.expression_string_equals(&equal_to);
            }
            _ => return Err(self.scanner.unsupported_next("Unsupported character for expression")),
        }

        self.scanner.expect_char(b')')?;
        Ok(())
    }

    fn create_property_ir(&mut self, name: &str) {
        self.ir.property(name);
        self.ir.down();
        self.max_level += 1;
    }

    fn create_index_ir(&mut self, index: i32) {
        self.ir.index(index);
        self.ir.down();
        self.max_level += 1;
    }

    fn read_property(&mut self) -> Result<String, JSONPathError> {
        let start_pos = self.scanner.position();
        while self.scanner.has_next() {
            let c = self.scanner.peek()?;
            if c == b' ' {
                return Err(self.scanner.error_next("Unexpected space"));
            } else if c == b'.' || c == b'[' {
                break;
            }
            self.scanner.next()?;
        }
        let end_pos = self.scanner.position();
        Ok(self.scanner.substring(start_pos, end_pos))
    }

    fn read_quoted_string(&mut self) -> Result<String, JSONPathError> {
        let quote = self.scanner.next()?;
        if quote != b'\'' && quote != b'"' {
            return Err(self.scanner.error("Invalid quoted string"));
        }

        let start_pos = self.scanner.position();
        let mut escaped = false;

        while self.scanner.has_next() {
            let c = self.scanner.peek()?;
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == quote {
                break;
            }
            self.scanner.next()?;
        }

        let end_pos = self.scanner.position();
        self.scanner.expect_char(quote)?;

        Ok(self.scanner.substring(start_pos, end_pos))
    }

    fn read_integer<F>(&mut self, is_end: F) -> Result<i32, JSONPathError>
    where
        F: Fn(u8) -> bool,
    {
        let start_pos = self.scanner.position();
        while self.scanner.has_next() {
            let c = self.scanner.peek()?;
            if (b'0'..=b'9').contains(&c) {
                self.scanner.next()?;
                continue;
            }
            if is_end(c) {
                break;
            }
            return Err(self.scanner.error("Invalid integer"));
        }
        let end_pos = self.scanner.position();
        let s = self.scanner.substring(start_pos, end_pos);
        s.parse::<i32>().map_err(|_| self.scanner.error("Invalid integer"))
    }
}
