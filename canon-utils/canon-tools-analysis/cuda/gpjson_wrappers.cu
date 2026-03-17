#include <cuda_runtime.h>

extern __global__ void create_combined_escape_carry_newline_count_index(char *file, long n, char *escape_carry_index, int *newline_count_index);
extern __global__ void create_combined_escape_newline_index(char *file, long n, bool *escape_carry_index, int *newline_count_index, long *escape_index, long escape_index_size, long *newline_index);
extern __global__ void create_quote_index(char *file, long n, long *escape_index, long *quote_index, char *quote_carry_index, long quote_index_size);
extern __global__ void create_string_index(long string_index_size, long *quote_index, char *quote_counts);
extern __global__ void create_leveled_bitmaps_carry_index(char *file, long n, long *string_index, char *level_carry_index);
extern __global__ void create_leveled_bitmaps(char *file, long n, long *string_index, char *carry_index, long *leveled_bitmaps_index, long leveled_bitmaps_index_size, long level_size, int num_levels);
extern __global__ void find_value(char *file, long n, long *new_line_index, long new_line_index_size, long *string_index, long *leveled_bitmaps_index, long leveled_bitmaps_index_size, long level_size, char *query, int result_size, long *result);

static const dim3 GRID_COMBINED(8);
static const dim3 BLOCK_COMBINED(1024);
static const dim3 GRID_FIND(512);
static const dim3 BLOCK_FIND(1024);

extern "C" int gpjson_cuda_malloc(void **out, size_t bytes) {
    return static_cast<int>(cudaMalloc(out, bytes));
}

extern "C" int gpjson_cuda_free(void *ptr) {
    return static_cast<int>(cudaFree(ptr));
}

extern "C" int gpjson_cuda_memcpy(void *dst, const void *src, size_t bytes, int kind) {
    return static_cast<int>(cudaMemcpy(dst, src, bytes, static_cast<cudaMemcpyKind>(kind)));
}

extern "C" int gpjson_cuda_memset(void *dst, int value, size_t bytes) {
    return static_cast<int>(cudaMemset(dst, value, bytes));
}

extern "C" int gpjson_cuda_device_synchronize() {
    return static_cast<int>(cudaDeviceSynchronize());
}

extern "C" int gpjson_create_combined_escape_carry_newline_count_index(char *file, long n, char *escape_carry_index, int *newline_count_index) {
    create_combined_escape_carry_newline_count_index<<<GRID_COMBINED, BLOCK_COMBINED>>>(file, n, escape_carry_index, newline_count_index);
    cudaError_t err = cudaGetLastError();
    if (err != cudaSuccess) {
        return static_cast<int>(err);
    }
    return static_cast<int>(cudaDeviceSynchronize());
}

extern "C" int gpjson_create_combined_escape_newline_index(char *file, long n, bool *escape_carry_index, int *newline_count_index, long *escape_index, long escape_index_size, long *newline_index) {
    create_combined_escape_newline_index<<<GRID_COMBINED, BLOCK_COMBINED>>>(file, n, escape_carry_index, newline_count_index, escape_index, escape_index_size, newline_index);
    cudaError_t err = cudaGetLastError();
    if (err != cudaSuccess) {
        return static_cast<int>(err);
    }
    return static_cast<int>(cudaDeviceSynchronize());
}

extern "C" int gpjson_create_quote_index(char *file, long n, long *escape_index, long *quote_index, char *quote_carry_index, long quote_index_size) {
    create_quote_index<<<GRID_COMBINED, BLOCK_COMBINED>>>(file, n, escape_index, quote_index, quote_carry_index, quote_index_size);
    cudaError_t err = cudaGetLastError();
    if (err != cudaSuccess) {
        return static_cast<int>(err);
    }
    return static_cast<int>(cudaDeviceSynchronize());
}

extern "C" int gpjson_create_string_index(long string_index_size, long *quote_index, char *quote_counts) {
    create_string_index<<<GRID_COMBINED, BLOCK_COMBINED>>>(string_index_size, quote_index, quote_counts);
    cudaError_t err = cudaGetLastError();
    if (err != cudaSuccess) {
        return static_cast<int>(err);
    }
    return static_cast<int>(cudaDeviceSynchronize());
}

extern "C" int gpjson_create_leveled_bitmaps_carry_index(char *file, long n, long *string_index, char *level_carry_index) {
    create_leveled_bitmaps_carry_index<<<GRID_COMBINED, BLOCK_COMBINED>>>(file, n, string_index, level_carry_index);
    cudaError_t err = cudaGetLastError();
    if (err != cudaSuccess) {
        return static_cast<int>(err);
    }
    return static_cast<int>(cudaDeviceSynchronize());
}

extern "C" int gpjson_create_leveled_bitmaps(char *file, long n, long *string_index, char *carry_index, long *leveled_bitmaps_index, long leveled_bitmaps_index_size, long level_size, int num_levels) {
    create_leveled_bitmaps<<<GRID_COMBINED, BLOCK_COMBINED>>>(file, n, string_index, carry_index, leveled_bitmaps_index, leveled_bitmaps_index_size, level_size, num_levels);
    cudaError_t err = cudaGetLastError();
    if (err != cudaSuccess) {
        return static_cast<int>(err);
    }
    return static_cast<int>(cudaDeviceSynchronize());
}

extern "C" int gpjson_find_value(char *file, long n, long *new_line_index, long new_line_index_size, long *string_index, long *leveled_bitmaps_index, long leveled_bitmaps_index_size, long level_size, char *query, int result_size, long *result) {
    find_value<<<GRID_FIND, BLOCK_FIND>>>(file, n, new_line_index, new_line_index_size, string_index, leveled_bitmaps_index, leveled_bitmaps_index_size, level_size, query, result_size, result);
    cudaError_t err = cudaGetLastError();
    if (err != cudaSuccess) {
        return static_cast<int>(err);
    }
    return static_cast<int>(cudaDeviceSynchronize());
}
