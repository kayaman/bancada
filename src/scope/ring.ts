// Power-of-two ring buffer over a Float32Array, addressed by absolute
// sample index (0 = first sample ever written). No allocation after
// construction.

export class RingBuffer {
  private readonly buf: Float32Array;
  private readonly mask: number;
  private total = 0;

  /** @param capacityPow2 capacity as an exponent: capacity = 2 ** capacityPow2 */
  constructor(capacityPow2: number) {
    if (!Number.isInteger(capacityPow2) || capacityPow2 < 0 || capacityPow2 > 26) {
      throw new Error(`RingBuffer: bad capacity exponent ${capacityPow2}`);
    }
    const cap = 1 << capacityPow2;
    this.buf = new Float32Array(cap);
    this.mask = cap - 1;
  }

  get capacity(): number {
    return this.buf.length;
  }

  /** Total samples ever written (monotonic). */
  get writtenTotal(): number {
    return this.total;
  }

  /** Absolute index of the oldest sample still held. */
  get firstAvailable(): number {
    return this.total > this.buf.length ? this.total - this.buf.length : 0;
  }

  write(values: Float32Array | number[]): void {
    const n = values.length;
    const buf = this.buf;
    const mask = this.mask;
    let w = this.total;
    for (let i = 0; i < n; i++) {
      buf[w & mask] = values[i];
      w++;
    }
    this.total = w;
  }

  /** Push a single sample. */
  push(value: number): void {
    this.buf[this.total & this.mask] = value;
    this.total++;
  }

  /**
   * Value at absolute sample index. Indices outside the retained history
   * return whatever currently occupies that slot (callers should clamp with
   * `firstAvailable` / `writtenTotal`).
   */
  get(i: number): number {
    return this.buf[i & this.mask];
  }

  /**
   * Copy `count` samples starting at absolute index `fromAbsIndex` into
   * `dst[0..]`, clamping the range to the available history.
   * Returns the number of samples actually copied.
   */
  readInto(dst: Float32Array, fromAbsIndex: number, count: number): number {
    let from = fromAbsIndex;
    const first = this.firstAvailable;
    if (from < first) {
      count -= first - from;
      from = first;
    }
    if (from + count > this.total) count = this.total - from;
    if (count > dst.length) count = dst.length;
    if (count <= 0) return 0;
    const buf = this.buf;
    const mask = this.mask;
    for (let i = 0; i < count; i++) {
      dst[i] = buf[(from + i) & mask];
    }
    return count;
  }

  clear(): void {
    this.total = 0;
    this.buf.fill(0);
  }
}
