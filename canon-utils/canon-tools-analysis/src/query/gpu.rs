use std::ffi::c_void;
use std::ptr;

#[cfg(feature = "cuda")]
extern "C" {
    fn gpjson_cuda_malloc(out: *mut *mut c_void, bytes: usize) -> i32;
    fn gpjson_cuda_free(ptr: *mut c_void) -> i32;
    fn gpjson_cuda_memcpy(dst: *mut c_void, src: *const c_void, bytes: usize, kind: i32) -> i32;
    fn gpjson_cuda_memset(dst: *mut c_void, value: i32, bytes: usize) -> i32;
    fn gpjson_cuda_device_synchronize() -> i32;

    fn gpjson_create_combined_escape_carry_newline_count_index(file: *mut i8, n: i64, escape_carry_index: *mut i8, newline_count_index: *mut i32) -> i32;

    fn gpjson_create_combined_escape_newline_index(
        file: *mut i8, n: i64, escape_carry_index: *mut u8, newline_count_index: *mut i32, escape_index: *mut i64, escape_index_size: i64, newline_index: *mut i64,
    ) -> i32;

    fn gpjson_create_quote_index(file: *mut i8, n: i64, escape_index: *mut i64, quote_index: *mut i64, quote_carry_index: *mut i8, quote_index_size: i64) -> i32;

    fn gpjson_create_string_index(string_index_size: i64, quote_index: *mut i64, quote_counts: *mut i8) -> i32;

    fn gpjson_create_leveled_bitmaps_carry_index(file: *mut i8, n: i64, string_index: *mut i64, level_carry_index: *mut i8) -> i32;

    fn gpjson_create_leveled_bitmaps(
        file: *mut i8, n: i64, string_index: *mut i64, carry_index: *mut i8, leveled_bitmaps_index: *mut i64, leveled_bitmaps_index_size: i64, level_size: i64, num_levels: i32,
    ) -> i32;

    fn gpjson_find_value(
        file: *mut i8, n: i64, new_line_index: *mut i64, new_line_index_size: i64, string_index: *mut i64, leveled_bitmaps_index: *mut i64, leveled_bitmaps_index_size: i64, level_size: i64,
        query: *mut i8, result_size: i32, result: *mut i64,
    ) -> i32;
}

pub const CUDA_MEMCPY_HOST_TO_DEVICE: i32 = 1;
pub const CUDA_MEMCPY_DEVICE_TO_HOST: i32 = 2;

#[derive(Debug)]
pub struct CudaError {
    pub code: i32,
    pub context: &'static str,
}

pub fn cuda_check(code: i32, context: &'static str) -> Result<(), CudaError> {
    if code == 0 {
        Ok(())
    } else {
        Err(CudaError { code, context })
    }
}

#[derive(Debug)]
pub struct DeviceBuffer {
    ptr: *mut c_void,
    pub bytes: usize,
}

unsafe impl Send for DeviceBuffer {}
unsafe impl Sync for DeviceBuffer {}

impl DeviceBuffer {
    pub fn new(bytes: usize) -> Result<Self, CudaError> {
        let mut out: *mut c_void = ptr::null_mut();
        unsafe {
            cuda_check(gpjson_cuda_malloc(&mut out as *mut *mut c_void, bytes), "cudaMalloc")?;
        }
        Ok(Self { ptr: out, bytes })
    }

    pub fn as_ptr(&self) -> *mut c_void {
        self.ptr
    }

    pub fn memset(&self, value: i32) -> Result<(), CudaError> {
        unsafe { cuda_check(gpjson_cuda_memset(self.ptr, value, self.bytes), "cudaMemset") }
    }

    pub fn copy_from_host(&self, src: *const c_void, bytes: usize) -> Result<(), CudaError> {
        unsafe { cuda_check(gpjson_cuda_memcpy(self.ptr, src, bytes, CUDA_MEMCPY_HOST_TO_DEVICE), "cudaMemcpy H2D") }
    }

    pub fn copy_to_host(&self, dst: *mut c_void, bytes: usize) -> Result<(), CudaError> {
        unsafe { cuda_check(gpjson_cuda_memcpy(dst, self.ptr, bytes, CUDA_MEMCPY_DEVICE_TO_HOST), "cudaMemcpy D2H") }
    }
}

impl Drop for DeviceBuffer {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                let _ = gpjson_cuda_free(self.ptr);
            }
            self.ptr = ptr::null_mut();
        }
    }
}

pub fn device_synchronize() -> Result<(), CudaError> {
    unsafe { cuda_check(gpjson_cuda_device_synchronize(), "cudaDeviceSynchronize") }
}

pub fn kernel_combined_escape_carry_newline_count_index(file: *mut i8, n: i64, escape_carry_index: *mut i8, newline_count_index: *mut i32) -> Result<(), CudaError> {
    unsafe { cuda_check(gpjson_create_combined_escape_carry_newline_count_index(file, n, escape_carry_index, newline_count_index), "create_combined_escape_carry_newline_count_index") }
}

pub fn kernel_combined_escape_newline_index(
    file: *mut i8, n: i64, escape_carry_index: *mut u8, newline_count_index: *mut i32, escape_index: *mut i64, escape_index_size: i64, newline_index: *mut i64,
) -> Result<(), CudaError> {
    unsafe {
        cuda_check(
            gpjson_create_combined_escape_newline_index(file, n, escape_carry_index, newline_count_index, escape_index, escape_index_size, newline_index),
            "create_combined_escape_newline_index",
        )
    }
}

pub fn kernel_create_quote_index(file: *mut i8, n: i64, escape_index: *mut i64, quote_index: *mut i64, quote_carry_index: *mut i8, quote_index_size: i64) -> Result<(), CudaError> {
    unsafe { cuda_check(gpjson_create_quote_index(file, n, escape_index, quote_index, quote_carry_index, quote_index_size), "create_quote_index") }
}

pub fn kernel_create_string_index(string_index_size: i64, quote_index: *mut i64, quote_counts: *mut i8) -> Result<(), CudaError> {
    unsafe { cuda_check(gpjson_create_string_index(string_index_size, quote_index, quote_counts), "create_string_index") }
}

pub fn kernel_create_leveled_bitmaps_carry_index(file: *mut i8, n: i64, string_index: *mut i64, level_carry_index: *mut i8) -> Result<(), CudaError> {
    unsafe { cuda_check(gpjson_create_leveled_bitmaps_carry_index(file, n, string_index, level_carry_index), "create_leveled_bitmaps_carry_index") }
}

pub fn kernel_create_leveled_bitmaps(
    file: *mut i8, n: i64, string_index: *mut i64, carry_index: *mut i8, leveled_bitmaps_index: *mut i64, leveled_bitmaps_index_size: i64, level_size: i64, num_levels: i32,
) -> Result<(), CudaError> {
    unsafe { cuda_check(gpjson_create_leveled_bitmaps(file, n, string_index, carry_index, leveled_bitmaps_index, leveled_bitmaps_index_size, level_size, num_levels), "create_leveled_bitmaps") }
}

pub fn kernel_find_value(
    file: *mut i8, n: i64, new_line_index: *mut i64, new_line_index_size: i64, string_index: *mut i64, leveled_bitmaps_index: *mut i64, leveled_bitmaps_index_size: i64, level_size: i64,
    query: *mut i8, result_size: i32, result: *mut i64,
) -> Result<(), CudaError> {
    unsafe {
        cuda_check(
            gpjson_find_value(file, n, new_line_index, new_line_index_size, string_index, leveled_bitmaps_index, leveled_bitmaps_index_size, level_size, query, result_size, result),
            "find_value",
        )
    }
}
