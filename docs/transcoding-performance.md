# Atlas conversion performance, 2026-10-21

The A380 has spare capacity. The deployed queue uses GPU encoding, but CPU
decoding and a single worker limit throughput. Full verification leaves the GPU
idle. The prepared update addresses all three limits without changing quality
presets or removing integrity checks.

## Hardware and live queue

- Intel Arc A380, PCI ID 8086:56a5, renderD128; AMD integrated GPU is renderD129.
- Ryzen 5 PRO 4650GE: six cores, twelve logical CPUs. No container CPU quota,
  affinity restriction or memory limit was configured.
- The current command selects `av1_vaapi`, CQP 28 and CPU input decoding with
  `-threads 2`. This CPU option does not restrict the fixed-function encoder to
  one GPU thread. One serial worker waits for encoding, both verification passes,
  hashes and publication before starting another file.
- During a ten-sample normal encoding observation, the Video engine averaged
  28.11% busy, peaking at 30.66%; Render/3D was idle. The process used about 293%
  CPU, with two CPU threads consuming nearly a full core each. Video-engine load
  is the useful GPU metric for this workload.
- The first 63-minute 4K item spent about 22 minutes encoding and 50 minutes in
  verification/cleanup. Its 7,964,788,868-byte source produced 9,724,091,723 bytes
  (22.1% larger), so the original was kept. The next job started automatically.

## Measured capacity

These were low-priority, bounded tests against Atlas's existing runtime. Encoded
output went to the null muxer; there were no saved benchmark media files. Real
input tests used the same short H.264 3840×2160, 23.976 fps segment. All jobs
returned successfully with the requested frame count. Production work continued.

| Input and path | Sessions | Frames per session | Wall seconds | Aggregate fps |
| --- | ---: | ---: | ---: | ---: |
| Synthetic 1080p, CPU-generated frames uploaded to AV1 encoder | 1 | 900 | 2.583 | 348.40 |
| Same | 2 | 900 | 3.023 | 595.49 |
| Same | 4 | 900 | 5.416 | 664.71 |
| Real 4K, CPU decode and upload to AV1 encoder | 1 | 600 | 8.996 | 66.70 |
| Real 4K, GPU decode directly into AV1 encoder | 1 | 600 | 4.613 | 130.07 |
| Same | 2 | 600 | 6.697 | 179.18 |
| Same | 4 | 600 | 12.991 | 184.74 |

Two GPU jobs nearly saturated this tested path; four offered only 3.1% more
aggregate throughput. The Video engine peaked at 99.53% during the benchmark
window. Start with two jobs. These figures exclude full verification, durable
publication and sustained storage contention; they are not a library completion
time guarantee. HEVC, other source codecs, quality settings, HDR and resolutions
have different costs. The 1080p rows measure encoding, not an entire transcode.

## Why AV1 checks could not use the GPU

The deployed `intel-media-va-driver` 25.2.3 is Intel's free-kernel build. An actual
AV1 decode returned VAEntryPoint unsupported, while AV1 encode succeeded.
Intel's [driver feature table](https://github.com/intel/media-driver#decodingencoding-features)
lists AV1 encode without decode for DG2 in that build. The
[full-feature Debian package](https://packages.debian.org/trixie/intel-media-va-driver-non-free)
supplies the missing functionality.

A disposable test container used the matching full-feature driver streamed into
RAM. It had no network or production mounts and removed itself after testing.
On the actual A380, both 8-bit and 10-bit 4K AV1 decoded all 240 generated frames
with fatal-error options in about one second. Truncating either stream yielded
158 frames. Decoder exit status alone accepted that shortened stream, so matching
complete frame totals remains mandatory. The production driver was untouched.

Both full Rust hardware workflows also passed on the actual Arc in a disposable
RAM-only synthetic stash: AV1/H.265 verified replacement and two parallel AV1
replacements. Instrumented FFmpeg calls confirmed four hardware input/encode
passes and eight hardware verification passes, all successful. Lossless H.264
uses a profile unsupported by the GPU; the separate lossless fixture run
confirmed CPU input fallback while retaining GPU encoding and output checks.
Supported-profile fixtures exercise the complete hardware path.

Intel specifies [two media engines and AV1/HEVC encode and decode](https://www.intel.com/content/www/us/en/products/sku/227959/intel-arc-a380-graphics/specifications.html)
for the A380. Intel's [concurrent encode guidance](https://www.intel.com/content/www/us/en/support/articles/000093450/graphics/intel-arc-dedicated-graphics-family.html)
sets no driver session cap and makes practical capacity dependent on the complete
system. The measurements above are a better bound for this configuration than a
generic stream-count claim.

## Prepared changes and delivery gate

- Two total workers, with the extra slot limited to tested hardware encoders.
- GPU decoding into GPU encoding surfaces; bounded software decoding fallback
  keeps the same GPU encoder when a source profile cannot use hardware decoding.
- GPU full-video verification on the encoder's render node. Full software
  fallback, all-audio decoding, matching nonzero frame totals, metadata and
  copied-stream hashes remain required. Hashing stays on the CPU.
- Full-feature Intel driver in amd64 runtime images; AMD drivers remain included.
- Discard an output that is not smaller before expensive verification.
- Independent worker journals, serialized claims/publication and a hard stop on
  new claims after unresolved recovery. No worker cleans up another live job.
- Live verification progress covers both files instead of staying at encoding's
  final percentage.

Deployment waits for the complete active queue, eligible candidates, staging
journals and downloads to drain, then passes the checks in `AGENTS.md` and the
isolated real-GPU Rust tests. No live conversion is cancelled to make a rollout
window. This investigation changed no production policies, media, deployment,
VPN settings or account data.
