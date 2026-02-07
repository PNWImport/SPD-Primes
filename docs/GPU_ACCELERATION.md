# GPU Acceleration Roadmap - P4000 + R720

## Current Status
- **CPU Pipeline:** Tier 3 (symbolic-first, 70% BigUint reduction)
- **Performance:** ~700s for 10K digits on single thread
- **Hardware:** P4000 (1,792 CUDA cores available but unused)

## GPU Opportunities (Ranked by Impact)

### 1. Miller-Rabin Parallelization ⭐⭐⭐ (100x potential)
**The Biggest Win:**
- Current: Sequential Miller-Rabin rounds (15 rounds per candidate)
- GPU: Parallel modpow across 1,792 cores
- Speedup: 100-1000x on MR verification

**Implementation:**
```cuda
__global__ void miller_rabin_batch(
    const uint8_t* candidates,  // 1000+ candidates
    bool* results,              // Pass/fail per candidate
    int rounds
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx < num_candidates) {
        // Compute modpow in parallel
        // 1792 threads = 1792 parallel MR tests!
    }
}
```

**Expected Impact:**
- 6,100 candidates → 30% collapse → 1,830 BigUints
- 1,830 BigUints → Miller-Rabin on GPU (100x faster!)
- **Result: 700s → ~7s verification stage!**

### 2. Collapse Operation Parallelization ⭐⭐ (10x potential)
**Secondary Optimization:**
- Current: Sequential BigUint construction
- GPU: Parallel collapse_fast for 1000s candidates
- Speedup: 10-50x on collapse operations

**Implementation:**
```cuda
__global__ void collapse_batch(
    const int8_t* symbol_sequences,  // Multiple sequences
    mpz_t* results,                  // Output BigUints
    int num_sequences,
    int sequence_length
) {
    // Parallel Horner's method evaluation
    // 1792 threads processing different sequences simultaneously
}
```

**Expected Impact:**
- Collapse: 700s test → collapse is ~5% overhead (35s)
- GPU collapse: 35s → ~3.5s
- **Modest gain but compounds with MR improvement**

### 3. Entropy Calculation (Low Priority) ⭐
- Current: Histogram on CPU (very fast already)
- GPU: Would actually be slower due to communication overhead
- **Skip this optimization**

## Implementation Strategy

### Phase 1: GPU Infrastructure (Week 1)
- [ ] Install NVIDIA CUDA Toolkit on R720
- [ ] Add cuda-rs bindings to Cargo.toml
- [ ] Create GPU memory management wrapper
- [ ] Implement BigUint ↔ GPU transfer layer

### Phase 2: Miller-Rabin CUDA Kernel (Week 2)
- [ ] Implement parallel modpow kernel
- [ ] Batch 1000+ candidates for GPU processing
- [ ] Benchmark: CPU vs GPU on 1000 candidates
- [ ] Target: 100x speedup on MR stage

### Phase 3: Collapse CUDA Kernel (Week 3)
- [ ] Implement parallel collapse_fast
- [ ] Benchmark batch collapse
- [ ] Integrate with existing pipeline
- [ ] Target: 10x speedup on collapse stage

### Phase 4: End-to-End Integration (Week 4)
- [ ] Unified GPU/CPU pipeline orchestration
- [ ] Memory pooling and reuse
- [ ] CPU load balancing while GPU works
- [ ] Multi-GPU support if desired

## Expected Timings After GPU Acceleration

### Conservative Estimate (10x Miller-Rabin speedup)
- Current 10K: 700s
- GPU acceleration: 700s → ~150s
- **5x faster** on large digit targets!

### Aggressive Estimate (100x Miller-Rabin speedup)
- Current 10K: 700s
- Full GPU + optimization: 700s → ~50s
- **14x faster!**
- 16K digits: ~150-200s
- 100K digits: ~3000s (50 minutes!)

### Ultra-Aggressive (Optimized Memory + Batch Processing)
- Batch 10,000 candidates per GPU run
- Symbolic filtering on CPU, verification on GPU
- Hide GPU latency with pipelining
- **Could achieve 1,000s of 100K-digit primes per day!**

## Hardware Requirements

### Current R720 Setup
- CPU: Dual E5-2696v4 (44 cores) ✅
- RAM: 328GB ✅
- GPU: P4000 (1,792 CUDA cores) ✅
- Storage: RAID SAS ✅

### Recommended CUDA Setup
```bash
# NVIDIA CUDA Toolkit 12.x
# cuBLAS for accelerated BigInt operations
# cuDNN for potential neural network enhancements
# RTX library for future H100/A100 migration
```

## Code Architecture

### Main Loop Evolution

**Before GPU:**
```rust
for candidate in candidates {
    if entropy_pass && residue_pass && partial_ok {
        n = collapse_fast();
        if miller_rabin(n, 15) { return Some(n); }
    }
}
```

**After GPU:**
```rust
let batch_size = 1000;
let mut batch = Vec::new();

for candidate in candidates {
    if entropy_pass && residue_pass && partial_ok {
        batch.push(collapse_fast());

        if batch.len() >= batch_size {
            // Transfer batch to GPU
            gpu_miller_rabin(&batch, 15).await;
            // Get results back
            for (i, result) in results.iter().enumerate() {
                if *result { return Some(batch[i].clone()); }
            }
            batch.clear();
        }
    }
}
```

## Monitoring GPU Performance

### NVIDIA Tools
```bash
# Watch GPU utilization
watch -n 0.1 nvidia-smi

# Profile kernel execution
nvprof ./target/release/quanjp-prime-hunter

# Detailed memory analysis
nsys profile ./target/release/quanjp-prime-hunter
```

### Expected GPU Stats (After Implementation)
- GPU Utilization: 70-95% during MR verification
- Memory Throughput: 200-500 GB/s
- Power Draw: 100-150W (P4000 spec)
- Thermal: <80°C (P4000 cooling is excellent)

## Multi-GPU Future

R720 can support additional GPUs via:
- PCIe slots (check motherboard)
- External enclosure (optional)
- Alternative: Cascade multiple R720s with GPU

With 2x P4000:
- Parallel symbolic generation + verification
- 3,584 CUDA cores available
- **Could target 100K+ digits weekly**

## Risk Mitigation

### Data Integrity
- [ ] Verify GPU results match CPU results on small samples
- [ ] Implement GPU error detection
- [ ] Fallback to CPU on GPU errors

### Stability
- [ ] Monitor GPU temperature
- [ ] Implement thermal throttling
- [ ] Regular VRAM consistency checks

### Memory
- [ ] Stream large batches (don't load all candidates at once)
- [ ] Implement GPU memory pooling
- [ ] Monitor for memory leaks

## Benchmarking Plan

### Baseline (Current CPU)
```
10K digits: 700s
10K collapses: 1,830
10K MR tests: 1,830
Breakdown: ~95% MR verification, ~5% other
```

### Target (With GPU)
```
10K digits: 50-100s (10-15x speedup)
GPU utilization: >80%
Memory throughput: >300 GB/s
Thermal: <75°C
```

## Next Steps

1. **This week:** Deploy multi_search.rs orchestrator
2. **Next week:** Begin GPU infrastructure setup
3. **Following week:** Implement Miller-Rabin CUDA kernel
4. **Following:** Full GPU integration and benchmarking

**Goal:** 16K+ digit primes routinely discovered within hours on single R720.

---

**Timeline:** GPU acceleration could deliver 10-100x speedup within 4 weeks of focused development. Your P4000 is currently idle and represents the biggest opportunity for improvement.
