// Native CUDA f64 probe: one block owns the complete dependent time line.
// Compile with --fmad=false; no approximate division/sqrt or changed force law.
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cuda_runtime.h>
#include <vector>
#define CHECK(x)                                                               \
  do {                                                                         \
    auto e = (x);                                                              \
    if (e != cudaSuccess) {                                                    \
      fprintf(stderr, "CUDA %s\n", cudaGetErrorString(e));                     \
      exit(1);                                                                 \
    }                                                                          \
  } while (0)
__device__ void force(int i, int n, const double *q, const double *gm,
                      double *a) {
  double ax = 0, ay = 0, az = 0;
  for (int k = 0; k < n; k++) {
    if (k == i)
      continue;
    int lo = k < i ? k : i, hi = k < i ? i : k;
    double dx = q[3 * hi] - q[3 * lo], dy = q[3 * hi + 1] - q[3 * lo + 1],
           dz = q[3 * hi + 2] - q[3 * lo + 2];
    double r2 = dx * dx + dy * dy + dz * dz;
    double s = gm[k] * (1.0 / (r2 * sqrt(r2)));
    if (k < i) {
      ax -= dx * s;
      ay -= dy * s;
      az -= dz * s;
    } else {
      ax += dx * s;
      ay += dy * s;
      az += dz * s;
    }
  }
  a[0] = ax;
  a[1] = ay;
  a[2] = az;
}
__device__ void pair_force(int n, const double *q, const double *gm,
                           double *matrix) {
  for (int index = threadIdx.x; index < n * n; index += blockDim.x) {
    int i = index / n, j = index % n;
    if (i >= j)
      continue;
    double dx = q[3 * j] - q[3 * i], dy = q[3 * j + 1] - q[3 * i + 1],
           dz = q[3 * j + 2] - q[3 * i + 2];
    double r2 = dx * dx + dy * dy + dz * dz, inv = 1.0 / (r2 * sqrt(r2));
    double si = gm[j] * inv, sj = gm[i] * inv;
    double d[3] = {dx, dy, dz};
    for (int c = 0; c < 3; c++) {
      matrix[3 * (i * n + j) + c] = d[c] * si;
      matrix[3 * (j * n + i) + c] = d[c] * sj;
    }
  }
  __syncthreads();
}
__device__ void sum_pairs(int i, int n, const double *matrix, double *a) {
  for (int c = 0; c < 3; c++)
    a[c] = 0;
  for (int j = 0; j < n; j++) {
    if (j == i)
      continue;
    for (int c = 0; c < 3; c++) {
      double value = matrix[3 * (i * n + j) + c];
      if (j < i)
        a[c] -= value;
      else
        a[c] += value;
    }
  }
}
template <bool Pairs>
__global__ void integrate(const double *input, double *samples, int n,
                          int steps) {
  extern __shared__ double shared[];
  samples += blockIdx.x * steps * n * 6;
  double *q = shared, *matrix = q + 3 * n;
  int i = threadIdx.x;
  double h = input[0];
  const double *w = input + 1, *gm = input + 16, *q0 = gm + n, *v0 = q0 + 3 * n;
  double v[3], a[3], comp[3] = {0, 0, 0};
  if (i < n) {
    for (int c = 0; c < 3; c++) {
      q[3 * i + c] = q0[3 * i + c];
      v[c] = v0[3 * i + c];
    }
  }
  __syncthreads();
  if constexpr (Pairs) {
    pair_force(n, q, gm, matrix);
    if (i < n)
      sum_pairs(i, n, matrix, a);
  } else {
    if (i < n)
      force(i, n, q, gm, a);
  }
  __syncthreads();
  for (int step = 0; step < steps; step++) {
    for (int sub = 0; sub < 15; sub++) {
      double half = 0.5 * w[sub] * h, drift = w[sub] * h;
      if (i < n) {
        for (int c = 0; c < 3; c++) {
          v[c] += a[c] * half;
          double y = v[c] * drift - comp[c], old = q[3 * i + c], next = old + y;
          comp[c] = (next - old) - y;
          q[3 * i + c] = next;
        }
      }
      __syncthreads();
      if constexpr (Pairs) {
        pair_force(n, q, gm, matrix);
        if (i < n)
          sum_pairs(i, n, matrix, a);
      } else {
        if (i < n)
          force(i, n, q, gm, a);
      }
      __syncthreads();
      if (i < n) {
        for (int c = 0; c < 3; c++)
          v[c] += a[c] * half;
      }
      __syncthreads();
    }
    if (i < n) {
      for (int c = 0; c < 3; c++) {
        samples[(step * n + i) * 6 + c] = q[3 * i + c];
        samples[(step * n + i) * 6 + 3 + c] = v[c];
      }
    }
  }
}
int main(int argc, char **argv) {
  if (argc < 2 || argc > 4)
    return 2;
  unsigned jobs = argc == 4 ? atoi(argv[3]) : 1;
  if (jobs == 0 || jobs > 32)
    return 2;
  if (argc >= 3 && strcmp(argv[2], "pairs") && strcmp(argv[2], "rows"))
    return 2;
  bool pairs = argc >= 3 && strcmp(argv[2], "pairs") == 0;
  FILE *f = fopen(argv[1], "rb");
  if (!f)
    return 2;
  unsigned n, steps;
  if (fread(&n, 4, 1, f) != 1 || fread(&steps, 4, 1, f) != 1)
    return 2;
  if (n > 1024 || steps > 30000)
    return 2;
  std::vector<double> input(16 + 7 * n), want(steps * n * 6),
      got(want.size() * jobs);
  if (fread(input.data(), 8, input.size(), f) != input.size() ||
      fread(want.data(), 8, want.size(), f) != want.size())
    return 2;
  fclose(f);
  double *dinput, *dout;
  CHECK(cudaMalloc(&dinput, input.size() * 8));
  CHECK(cudaMalloc(&dout, got.size() * 8));
  CHECK(cudaMemcpy(dinput, input.data(), input.size() * 8,
                   cudaMemcpyHostToDevice));
  cudaEvent_t start, end;
  CHECK(cudaEventCreate(&start));
  CHECK(cudaEventCreate(&end));
  std::vector<float> timings;
  double wall = 0;
  for (int run = 0; run < 8; run++) {
    auto t = std::chrono::steady_clock::now();
    CHECK(cudaEventRecord(start));
    if (pairs) {
      CHECK(cudaFuncSetAttribute(integrate<true>,
                                 cudaFuncAttributeMaxDynamicSharedMemorySize,
                                 (3 * n + 3 * n * n) * 8));
      integrate<true>
          <<<jobs, 512, (3 * n + 3 * n * n) * 8>>>(dinput, dout, n, steps);
    } else {
      integrate<false>
          <<<jobs, ((n + 31) / 32) * 32, 3 * n * 8>>>(dinput, dout, n, steps);
    }
    CHECK(cudaGetLastError());
    CHECK(cudaEventRecord(end));
    CHECK(cudaEventSynchronize(end));
    float ms;
    CHECK(cudaEventElapsedTime(&ms, start, end));
    CHECK(cudaMemcpy(got.data(), dout, got.size() * 8, cudaMemcpyDeviceToHost));
    if (run > 0) {
      timings.push_back(ms);
      wall +=
          std::chrono::duration<double>(std::chrono::steady_clock::now() - t)
              .count();
    }
  }
  size_t mismatch = 0;
  double maxpos = 0, maxvel = 0;
  for (size_t j = 0; j < got.size(); j++) {
    if (memcmp(&got[j], &want[j % want.size()], 8))
      mismatch++;
    double error = abs(got[j] - want[j % want.size()]);
    if (j % 6 < 3)
      maxpos = fmax(maxpos, error);
    else
      maxvel = fmax(maxvel, error);
  }
  printf("{\"jobs\":%u,\"pairs\":%s,\"bodies\":%u,\"steps\":%u,\"mismatched_"
         "values\":%zu,\"max_position_component_m\":%.17g,\"max_velocity_"
         "component_m_s\":%.17g,\"wall_with_readback_mean_s\":%.9g,\"kernel_"
         "ms\":[",
         jobs, pairs ? "true" : "false", n, steps, mismatch, maxpos, maxvel,
         wall / 7);
  for (size_t j = 0; j < timings.size(); j++)
    printf("%s%.6f", j ? "," : "", timings[j]);
  puts("]}");
  CHECK(cudaFree(dinput));
  CHECK(cudaFree(dout));
  return 0;
}
