mod gpu;
mod jsonpath;
pub mod consumer;
pub use consumer::QueryConsumer;
use gpu::{
    device_synchronize, kernel_combined_escape_carry_newline_count_index,
    kernel_combined_escape_newline_index, kernel_create_leveled_bitmaps,
    kernel_create_leveled_bitmaps_carry_index, kernel_create_quote_index,
    kernel_create_string_index, kernel_find_value, CudaError, DeviceBuffer,
};
use jsonpath::{JSONPathError, JSONPathParser};
use std::ffi::c_void;
use std::fs;
use std::path::Path;
use std::sync::Arc;

const GRID_SIZE: usize = 8;
const BLOCK_SIZE: usize = 1024;
const CARRY_INDEX_SIZE: usize = GRID_SIZE * BLOCK_SIZE;
const MAX_NUM_LEVELS: usize = 16;

#[derive(Debug, Clone, Copy, Default)]
pub struct QueryOptions {
    pub disable_fallback: bool,
}

#[derive(Debug)]
pub enum QueryError {
    Io(std::io::Error),
    JsonPath(JSONPathError),
    InvalidQueryInput(String),
    Cuda(CudaError),
    Unsupported(String),
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryError::Io(err) => write!(f, "io error: {err}"),
            QueryError::JsonPath(err) => write!(f, "jsonpath error: {err}"),
            QueryError::InvalidQueryInput(msg) => write!(f, "invalid query input: {msg}"),
            QueryError::Cuda(err) => write!(f, "cuda error ({}): {}", err.code, err.context),
            QueryError::Unsupported(msg) => write!(f, "unsupported: {msg}"),
        }
    }
}

impl std::error::Error for QueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            QueryError::Io(err) => Some(err),
            QueryError::JsonPath(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for QueryError {
    fn from(err: std::io::Error) -> Self {
        QueryError::Io(err)
    }
}

impl From<JSONPathError> for QueryError {
    fn from(err: JSONPathError) -> Self {
        QueryError::JsonPath(err)
    }
}

impl From<CudaError> for QueryError {
    fn from(err: CudaError) -> Self {
        QueryError::Cuda(err)
    }
}

#[derive(Debug, Clone)]
pub struct TlogQueryResult {
    file: Arc<Vec<u8>>,
    pub number_of_lines: usize,
    pub results_per_line: usize,
    pub values: Vec<i64>,
}

impl TlogQueryResult {
    pub fn value(&self, line_index: usize, result_index: usize) -> Option<String> {
        if line_index >= self.number_of_lines {
            return None;
        }
        if result_index >= self.results_per_line {
            return None;
        }

        let idx = line_index * 2 * self.results_per_line + result_index * 2;
        let start = self.values[idx];
        if start < 0 {
            return None;
        }
        let end = self.values[idx + 1];
        let start = start as usize;
        let end = end as usize;
        if end <= start || end > self.file.len() {
            return None;
        }
        let slice = &self.file[start..end];
        Some(String::from_utf8_lossy(slice).to_string())
    }

    pub fn line_values(&self, line_index: usize) -> Vec<Option<String>> {
        let mut out = Vec::with_capacity(self.results_per_line);
        for i in 0..self.results_per_line {
            out.push(self.value(line_index, i));
        }
        out
    }
}

pub fn query_file<P: AsRef<Path>>(
    path: P,
    queries: &[String],
    _options: QueryOptions,
) -> Result<Vec<TlogQueryResult>, QueryError> {
    if queries.is_empty() {
        return Err(QueryError::InvalidQueryInput(
            "expected at least one query".to_string(),
        ));
    }

    let file = fs::read(path)?;
    let file_len = file.len() as i64;
    if file_len == 0 {
        return Err(QueryError::InvalidQueryInput(
            "input file is empty".to_string(),
        ));
    }

    let mut compiled = Vec::with_capacity(queries.len());
    let mut max_depth = 0usize;
    let mut max_query_size = 0usize;
    let mut max_results = 0usize;

    for query in queries {
        let result = JSONPathParser::new(query).compile()?;
        max_depth = max_depth.max(result.max_depth);
        max_query_size = max_query_size.max(result.ir.len());
        max_results = max_results.max(result.num_results);
        compiled.push(result);
    }

    if max_depth == 0 {
        return Err(QueryError::InvalidQueryInput(
            "query produced zero depth".to_string(),
        ));
    }

    if max_depth > MAX_NUM_LEVELS {
        return Err(QueryError::Unsupported(format!(
            "max depth {} exceeds MAX_NUM_LEVELS {}",
            max_depth, MAX_NUM_LEVELS
        )));
    }

    let file_arc = Arc::new(file);

    let file_device = DeviceBuffer::new(file_arc.len())?;
    file_device.copy_from_host(file_arc.as_ptr() as *const c_void, file_arc.len())?;

    let result_size = ((file_arc.len() as i64 + 64 - 1) / 64) as usize;

    let string_index_device = DeviceBuffer::new(result_size * 8)?;
    string_index_device.memset(0)?;
    let escape_carry_device = DeviceBuffer::new(CARRY_INDEX_SIZE)?;
    let escape_index_device = DeviceBuffer::new(result_size * 8)?;
    let newline_count_device = DeviceBuffer::new(CARRY_INDEX_SIZE * 4)?;

    escape_index_device.memset(0)?;

    kernel_combined_escape_carry_newline_count_index(
        file_device.as_ptr() as *mut i8,
        file_len,
        escape_carry_device.as_ptr() as *mut i8,
        newline_count_device.as_ptr() as *mut i32,
    )?;

    let mut newline_counts = vec![0i32; CARRY_INDEX_SIZE];
    newline_count_device.copy_to_host(
        newline_counts.as_mut_ptr() as *mut c_void,
        newline_counts.len() * 4,
    )?;

    let mut sum = 1i32;
    for value in newline_counts.iter_mut() {
        let current = *value;
        *value = sum;
        sum = sum.saturating_add(current);
    }

    newline_count_device.copy_from_host(
        newline_counts.as_ptr() as *const c_void,
        newline_counts.len() * 4,
    )?;

    if sum <= 0 {
        return Err(QueryError::InvalidQueryInput(
            "failed to compute newline index size".to_string(),
        ));
    }

    let number_of_lines = sum as usize;

    let newline_index_device = DeviceBuffer::new(number_of_lines * 8)?;
    newline_index_device.memset(0)?;

    kernel_combined_escape_newline_index(
        file_device.as_ptr() as *mut i8,
        file_len,
        escape_carry_device.as_ptr() as *mut u8,
        newline_count_device.as_ptr() as *mut i32,
        escape_index_device.as_ptr() as *mut i64,
        result_size as i64,
        newline_index_device.as_ptr() as *mut i64,
    )?;

    kernel_create_quote_index(
        file_device.as_ptr() as *mut i8,
        file_len,
        escape_index_device.as_ptr() as *mut i64,
        string_index_device.as_ptr() as *mut i64,
        escape_carry_device.as_ptr() as *mut i8,
        result_size as i64,
    )?;

    let mut carry_buffer = vec![0u8; CARRY_INDEX_SIZE];
    escape_carry_device.copy_to_host(
        carry_buffer.as_mut_ptr() as *mut c_void,
        carry_buffer.len(),
    )?;

    let mut previous_value: u8 = 0;
    for value in carry_buffer.iter_mut() {
        let new_value = *value ^ previous_value;
        *value = new_value;
        previous_value = new_value;
    }

    escape_carry_device.copy_from_host(
        carry_buffer.as_ptr() as *const c_void,
        carry_buffer.len(),
    )?;

    kernel_create_string_index(
        result_size as i64,
        string_index_device.as_ptr() as *mut i64,
        escape_carry_device.as_ptr() as *mut i8,
    )?;

    let level_size = ((file_arc.len() as i64 + 64 - 1) / 64) as i64;
    let leveled_bitmaps_size = (level_size as usize) * max_depth;

    let leveled_bitmaps_device = DeviceBuffer::new(leveled_bitmaps_size * 8)?;
    leveled_bitmaps_device.memset(0)?;

    let level_carry_device = DeviceBuffer::new(CARRY_INDEX_SIZE)?;

    kernel_create_leveled_bitmaps_carry_index(
        file_device.as_ptr() as *mut i8,
        file_len,
        string_index_device.as_ptr() as *mut i64,
        level_carry_device.as_ptr() as *mut i8,
    )?;

    let mut level_carry = vec![0i8; CARRY_INDEX_SIZE];
    level_carry_device.copy_to_host(
        level_carry.as_mut_ptr() as *mut c_void,
        level_carry.len(),
    )?;

    let mut level: i8 = -1;
    for value in level_carry.iter_mut() {
        let current = *value;
        *value = level;
        level = level.wrapping_add(current);
    }

    level_carry_device.copy_from_host(
        level_carry.as_ptr() as *const c_void,
        level_carry.len(),
    )?;

    kernel_create_leveled_bitmaps(
        file_device.as_ptr() as *mut i8,
        file_len,
        string_index_device.as_ptr() as *mut i64,
        level_carry_device.as_ptr() as *mut i8,
        leveled_bitmaps_device.as_ptr() as *mut i64,
        leveled_bitmaps_size as i64,
        level_size,
        max_depth as i32,
    )?;

    device_synchronize()?;

    let results_per_line = max_results.max(1);
    let result_len = number_of_lines * 2 * results_per_line;

    let result_device = DeviceBuffer::new(result_len * 8)?;
    let query_device = DeviceBuffer::new(max_query_size.max(1))?;

    let mut outputs = Vec::with_capacity(compiled.len());

    for query in compiled.iter() {
        result_device.memset(0xFF)?; // -1 for i64
        query_device.copy_from_host(query.ir.as_ptr() as *const c_void, query.ir.len())?;

        kernel_find_value(
            file_device.as_ptr() as *mut i8,
            file_len,
            newline_index_device.as_ptr() as *mut i64,
            number_of_lines as i64,
            string_index_device.as_ptr() as *mut i64,
            leveled_bitmaps_device.as_ptr() as *mut i64,
            leveled_bitmaps_size as i64,
            level_size,
            query_device.as_ptr() as *mut i8,
            results_per_line as i32,
            result_device.as_ptr() as *mut i64,
        )?;

        let mut host_values = vec![0i64; result_len];
        result_device.copy_to_host(
            host_values.as_mut_ptr() as *mut c_void,
            host_values.len() * 8,
        )?;

        outputs.push(TlogQueryResult {
            file: Arc::clone(&file_arc),
            number_of_lines,
            results_per_line,
            values: host_values,
        });
    }

    Ok(outputs)
}

pub fn query_file_single<P: AsRef<Path>>(
    path: P,
    query: &str,
    options: QueryOptions,
) -> Result<TlogQueryResult, QueryError> {
    let results = query_file(path, &[query.to_string()], options)?;
    Ok(results.into_iter().next().unwrap())
}
