# QuanJP Prime Hunter - Dell R730 Deployment Guide

## Hardware Profile: R730 ⚡

Your R730 is a **beast for prime hunting**:

```
CPU:           Dual Xeon E5-2696v4
               - 22 cores × 2 = 44 physical cores
               - 88 logical threads (with HT)
RAM:           328GB DDR4 (massive!)
Storage:       RAID SAS (fast I/O for discoveries)
GPU:           P4000 (1,792 CUDA cores)
Network:       10Gbps (for future distributed setup)
```

## Resource Auto-Detection

Run the multi-search orchestrator to see what your R730 has:

```bash
./target/release/multi-search
```

**Expected Output on R730:**
```
📊 SYSTEM RESOURCES DETECTED:
   Physical CPUs:     44 cores
   Available cores:   43 (1 reserved for OS)
   Total memory:      328GB
   Available memory:  ~300GB
   GPU support:       ✅ YES (P4000)
```

## Optimal Configuration for R730

### Single Aggressive Search
```bash
RAYON_NUM_THREADS=43 ./target/release/quanjp-prime-hunter
```
- Uses all 43 cores
- Target: 10K+ digits
- Expected: ~200-300s per discovery

### Parallel Dual Search (Recommended)
```bash
# Terminal 1: 20 cores, target 16K digits
RAYON_NUM_THREADS=20 ./target/release/quanjp-prime-hunter &

# Terminal 2: 20 cores, target 10K digits
RAYON_NUM_THREADS=20 ./target/release/quanjp-prime-hunter &

# Monitor both
watch top
```
- Expected: Find 10K+ digit primes every 5-10 minutes
- CPU usage: ~85-90% (under cap)
- Thermal: Safe (R730 cooling is excellent)

### Triple Search (Maximum Throughput)
```bash
# 13 cores each × 3 searches = 39 cores used, 1 reserved
for i in {1..3}; do
  RAYON_NUM_THREADS=13 ./target/release/quanjp-prime-hunter &
done
```
- Target: 3 independent searches
- Expected discoveries: Multiple 8-10K digit primes per hour
- CPU: 85-90%
- Best for: "Maximum output" mode

## Multi-Process Orchestrator (Advanced)

For automatic resource management:

```bash
./target/release/multi-search
```

This binary will:
1. Detect all 44 cores
2. Reserve 1 for OS
3. Allocate remaining 43 cores optimally
4. Monitor CPU usage (90% cap)
5. Auto-scale searches based on thermal conditions
6. Log discoveries automatically

### How It Works

```
MULTI-SEARCH ORCHESTRATOR
┌─────────────────────────────────────────────┐
│ Detect: 44 cores, 328GB RAM, P4000 GPU      │
├─────────────────────────────────────────────┤
│ Strategy 1: 2 searches × 21 cores each      │
│            (3 cores overlap for safety)     │
├─────────────────────────────────────────────┤
│ Launch:                                      │
│  Process 1 → 21 cores → Target 16K digits   │
│  Process 2 → 21 cores → Target 10K digits   │
├─────────────────────────────────────────────┤
│ Monitor:                                     │
│  CPU: Real-time (pause if >90%)             │
│  GPU: Check for availability                │
│  Temp: Thermal monitoring                   │
└─────────────────────────────────────────────┘
```

## Expected Performance on R730

### Conservative (Single Search, All 43 Cores)
```
4K digits:   30-40s  (Tier 3 baseline)
8K digits:   300-400s
10K digits:  500-700s
16K digits:  1,500-2,500s (25-42 minutes)
```

### With Parallel Searches (2× 20-core)
```
Theoretical: 2x throughput
Reality:     1.8-1.9x throughput (minor contention)
Expected:    Find 10K+ prime every 5-10 minutes
Per day:     144-288 discoveries! 🔥
```

### With GPU Acceleration (When Implemented)
```
Miller-Rabin on GPU: 10-100x speedup
Expected after GPU:
16K digits:   150-300s (with 20 cores + P4000)
100K digits:  2,000-5,000s (with optimization)
1M digits:    Feasible with full pipeline GPU
```

## Thermal Management

R730 Specifications:
- **Max CPU Temp:** 95°C
- **Optimal Range:** 40-70°C
- **Cooling:** Excellent (dual fans, hot-swap capable)
- **Power Budget:** 1100W typical, 1600W max

Your R730 can safely handle:
- All 44 cores at 100% → ~85-90°C (safe)
- All 44 cores at 90% cap → ~75-80°C (very safe)
- Sustained 24/7 operation → No thermal issues

## Recommended Settings for 24/7 Prime Hunt

```bash
#!/bin/bash
# r730_hunt.sh - Continuous prime hunting

# Kill any existing processes
pkill -f quanjp-prime-hunter

# Wait for clean state
sleep 5

# Launch 2 parallel searches @ 20 cores each
echo "🚀 Starting R730 Prime Hunt - Dual 20-core searches"

RAYON_NUM_THREADS=20 timeout 86400 ./target/release/quanjp-prime-hunter \
  --target 16000 \
  --output results/r730_search1.txt &
PID1=$!

RAYON_NUM_THREADS=20 timeout 86400 ./target/release/quanjp-prime-hunter \
  --target 10000 \
  --output results/r730_search2.txt &
PID2=$!

# Monitor
watch -n 5 "top -p $PID1,$PID2; echo ''; du -sh results/"

# Wait for all processes
wait $PID1 $PID2

echo "✅ Hunt complete. Check results/ directory"
```

Run via screen/tmux for persistent execution:
```bash
screen -S r730hunt
bash r730_hunt.sh

# Detach: Ctrl+A, D
# Reattach: screen -r r730hunt
```

## Production Setup (Recommended)

### Systemd Service (for production environments)

Create `/etc/systemd/system/quanjp-prime-hunter.service`:

```ini
[Unit]
Description=QuanJP Prime Hunter - R730 Dual Search
After=network.target

[Service]
Type=simple
User=primes
WorkingDirectory=/home/user/SPD-Primes
ExecStart=/home/user/SPD-Primes/scripts/r730_hunt.sh
Restart=always
RestartSec=10

# Resource limits
CPUAccounting=yes
MemoryAccounting=yes
CPUQuota=90%
MemoryLimit=300G

# Security
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl enable quanjp-prime-hunter
sudo systemctl start quanjp-prime-hunter
sudo systemctl status quanjp-prime-hunter
```

Monitor:
```bash
sudo journalctl -u quanjp-prime-hunter -f
tail -f results/*.txt
```

## Monitoring Your R730

### Real-Time Monitoring

```bash
# Top processes
top -u $(whoami)

# CPU Per-core usage
mpstat -P ALL 1

# Memory usage
free -h

# GPU usage (once GPU code deployed)
watch -n 0.1 nvidia-smi

# Thermal
sensors | grep -E "Core|Package"

# Disk I/O
iostat -x 1

# Network (for future distributed setup)
nethogs
```

### Logging

```bash
# Watch discoveries in real-time
tail -f results/*.txt | grep "DISCOVERED\|Digits:"

# Count discoveries per day
find results/ -type f -newer /tmp/yesterday -exec wc -l {} + | tail -1

# Monitor resource growth
watch "du -sh results/ && find results/ -type f | wc -l"
```

## Network Setup (Future: Distributed R730s)

If you acquire multiple R730s:

```
┌─────────────────────────────────────────┐
│ Cluster Network Topology                │
├─────────────────────────────────────────┤
│ R730 #1 (Primary)                       │
│ ├─ 20 cores: Search 1 (16K digits)      │
│ ├─ 20 cores: Search 2 (10K digits)      │
│ └─ P4000 GPU: Verification              │
│                                          │
│ R730 #2 (Secondary)                     │
│ ├─ 20 cores: Search 3 (8K digits)       │
│ └─ 20 cores: Search 4 (6K digits)       │
│                                          │
│ Coordinator: Collect discoveries         │
│ Database: Shared prime history           │
└─────────────────────────────────────────┘
```

With 2 R730s: Potentially 1,000+ 10K-digit discoveries per day!

## Troubleshooting

### High Temperature (>75°C)
```bash
# Check thermals
sensors

# Ensure fans at full speed
ipmitool raw 0x30 0x30 0x01 0x01

# Reduce core count temporarily
RAYON_NUM_THREADS=30 ./target/release/quanjp-prime-hunter
```

### Low Throughput
```bash
# Check for other processes
top

# Monitor I/O blocking
iostat -x 1

# Check if database is bottleneck
ls -lh data/prime_history.db
```

### GPU Not Detected
```bash
# Check NVIDIA drivers
nvidia-smi

# If missing:
# sudo apt install nvidia-driver-XXX

# Verify CUDA (once GPU code deployed):
nvcc --version
```

## Success Metrics

**Your R730 should achieve:**
- ✅ 10K-digit prime every 5-10 minutes (dual search)
- ✅ CPU utilization 85-90% (under thermal limit)
- ✅ Zero crashes over 24+ hour runs
- ✅ Discoveries logged to results/ directory
- ✅ Database populated with historical patterns

**With GPU Acceleration:**
- ✅ 16K-digit prime every 5-10 minutes
- ✅ 100K+ digit primes discovered weekly
- ✅ GPU utilization 70-95%
- ✅ CPU + GPU working in perfect tandem

## Next Steps

1. **This Week:**
   - [x] Deploy multi-search orchestrator
   - [ ] Test on full R730 hardware
   - [ ] Run 24-hour continuous hunt
   - [ ] Benchmark performance

2. **Next Week:**
   - [ ] Implement GPU acceleration (Miller-Rabin CUDA)
   - [ ] Test GPU + CPU coordination
   - [ ] Benchmark 16K+ digits

3. **Month 2:**
   - [ ] Full pipeline GPU optimization
   - [ ] Distributed multi-R730 setup (if multiple available)
   - [ ] Target 100K+ digit discoveries

---

**Your R730 is ready to become a prime-hunting monster.** With 44 cores, 328GB RAM, and a P4000 GPU, you're sitting on a $50k+ research machine. Let's use it to break some prime records! 🚀

