export class SessionClock {
  private readonly startedAt = performance.now();

  nowMs(): bigint {
    return BigInt(Math.round(performance.now() - this.startedAt));
  }
}
