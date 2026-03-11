import { useState } from "react";

const MAX_GAP_MS = 1600;

function GapBar({ label, currentMs, targetMs }: { label: string; currentMs: number; targetMs: number }) {
  const currentPct = Math.min(100, (currentMs / MAX_GAP_MS) * 100);
  const targetPct = Math.min(99, (targetMs / MAX_GAP_MS) * 100);
  const multiplier = Math.round(currentMs / targetMs);
  return (
    <div className="pa-gap-row">
      <span className="pa-gap-label">{label}</span>
      <div className="pa-gap-bar-wrap">
        <div className="pa-gap-bar-bg">
          <div className="pa-gap-bar-fill" style={{ width: `${currentPct}%` }} />
          <div className="pa-gap-target-line" style={{ left: `${targetPct}%` }} />
        </div>
      </div>
      <span className="pa-gap-stats">
        <span className="pa-gap-current">{currentMs}ms</span>
        <span className="pa-gap-arrow"> → </span>
        <span className="pa-gap-target-val">{targetMs}ms</span>
        <span className="pa-gap-mult"> ({multiplier}x)</span>
      </span>
    </div>
  );
}

function Collapsible({ title, children }: { title: string; children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="pa-collapsible">
      <button className="pa-toggle" onClick={() => setOpen((v) => !v)}>
        <span className="pa-chevron">{open ? "▾" : "▸"}</span>
        {title}
      </button>
      {open && <div className="pa-content">{children}</div>}
    </div>
  );
}

export function PipelineAnalysis() {
  const [open, setOpen] = useState(false);

  return (
    <div className="pipeline-analysis">
      <button className="pa-main-toggle" onClick={() => setOpen((v) => !v)}>
        <span className="pa-main-chevron">{open ? "▾" : "▸"}</span>
        Pipeline Analysis
        {!open && <span className="pa-main-hint"> — my choices vs Brivva's pipeline, gap to 300ms</span>}
      </button>

      {open && (
        <div className="pa-body">
          {/* My Pipeline vs Brivva's */}
          <div className="pa-section">
            <h4 className="pa-subtitle">My Prototype vs Brivva's Pipeline</h4>
            <div className="pa-compare">
              <div className="pa-col">
                <div className="pa-col-header pa-col-mine">My Prototype</div>
                <table className="pa-table">
                  <thead>
                    <tr>
                      <th>Phase</th>
                      <th>Tech</th>
                      <th>Measured</th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr>
                      <td>STT</td>
                      <td>Deepgram Nova-3</td>
                      <td>streaming ✓</td>
                    </tr>
                    <tr>
                      <td>Translation</td>
                      <td>M2M100-1.2B (CF)</td>
                      <td>394–870ms</td>
                    </tr>
                    <tr>
                      <td>TTS</td>
                      <td>Kokoro 82M (MPS)</td>
                      <td>430–2300ms</td>
                    </tr>
                    <tr>
                      <td>Rooms</td>
                      <td>Durable Objects</td>
                      <td>fan-out/lang</td>
                    </tr>
                    <tr>
                      <td>Lip-sync</td>
                      <td>—</td>
                      <td className="pa-muted">not built</td>
                    </tr>
                  </tbody>
                </table>
              </div>
              <div className="pa-col">
                <div className="pa-col-header pa-col-brivva">Brivva's Pipeline</div>
                <table className="pa-table">
                  <thead>
                    <tr>
                      <th>Phase</th>
                      <th>Tech</th>
                      <th>Status</th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr>
                      <td>STT</td>
                      <td>Whisper</td>
                      <td className="pa-warn">batch only</td>
                    </tr>
                    <tr>
                      <td>Translation</td>
                      <td>Context NMT</td>
                      <td className="pa-muted">unknown</td>
                    </tr>
                    <tr>
                      <td>TTS</td>
                      <td>Emotive TTS (3B)</td>
                      <td className="pa-muted">unknown</td>
                    </tr>
                    <tr>
                      <td>Lip-sync</td>
                      <td>Wav2Lip</td>
                      <td className="pa-muted">lips only</td>
                    </tr>
                    <tr>
                      <td>Target</td>
                      <td>—</td>
                      <td className="pa-good">&lt;300ms e2e</td>
                    </tr>
                  </tbody>
                </table>
              </div>
            </div>
          </div>

          {/* Component Decisions */}
          <div className="pa-section">
            <h4 className="pa-subtitle">Component Decisions</h4>

            <Collapsible title="STT — Why Nova-3 over Whisper">
              <table className="pa-table">
                <thead>
                  <tr>
                    <th>Model</th>
                    <th>WER</th>
                    <th>Streaming</th>
                    <th>Price</th>
                  </tr>
                </thead>
                <tbody>
                  <tr className="pa-winner">
                    <td>✓ Nova-3</td>
                    <td>6.5%</td>
                    <td className="pa-good">Yes — live interims</td>
                    <td>$4.30/1000min</td>
                  </tr>
                  <tr>
                    <td>Whisper v3 Turbo</td>
                    <td>4.8%</td>
                    <td className="pa-warn">No — batch only</td>
                    <td>$0.67/1000min</td>
                  </tr>
                </tbody>
              </table>
              <p className="pa-verdict">
                Streaming is non-negotiable for live translation. Whisper is better for batch/analytics.
              </p>
            </Collapsible>

            <Collapsible title="Translation — Why M2M100 over LLM">
              <table className="pa-table">
                <thead>
                  <tr>
                    <th>Model</th>
                    <th>Type</th>
                    <th>Latency</th>
                  </tr>
                </thead>
                <tbody>
                  <tr className="pa-winner">
                    <td>✓ M2M100-1.2B</td>
                    <td>Seq2Seq</td>
                    <td className="pa-good">394–870ms</td>
                  </tr>
                  <tr>
                    <td>Llama 3.2 1B</td>
                    <td>LLM prompted</td>
                    <td className="pa-warn">~1,500ms</td>
                  </tr>
                </tbody>
              </table>
              <p className="pa-verdict">
                Dedicated translation model. 3x faster than prompting an LLM.
              </p>
            </Collapsible>

            <Collapsible title="TTS — Why Kokoro (and its limits)">
              <table className="pa-table">
                <thead>
                  <tr>
                    <th>Model</th>
                    <th>Params</th>
                    <th>Latency</th>
                    <th>Expressive</th>
                  </tr>
                </thead>
                <tbody>
                  <tr className="pa-winner">
                    <td>✓ Kokoro (self-hosted)</td>
                    <td>82M</td>
                    <td className="pa-good">430–2300ms</td>
                    <td className="pa-warn">Limited</td>
                  </tr>
                  <tr>
                    <td>Kokoro (Replicate)</td>
                    <td>82M</td>
                    <td className="pa-bad">2,000–14,000ms</td>
                    <td className="pa-warn">Limited</td>
                  </tr>
                  <tr>
                    <td>Emotive TTS (Brivva)</td>
                    <td>3B</td>
                    <td className="pa-muted">unknown</td>
                    <td className="pa-good">Yes — emotions</td>
                  </tr>
                </tbody>
              </table>
              <p className="pa-verdict">
                Kokoro wins for demo speed. Production needs expressive TTS for live commerce energy.
              </p>
            </Collapsible>
          </div>

          {/* Gap to 300ms */}
          <div className="pa-section">
            <h4 className="pa-subtitle">Gap to 300ms Target</h4>
            <div className="pa-gap-bars">
              <GapBar label="Translate" currentMs={630} targetMs={50} />
              <GapBar label="TTS" currentMs={1340} targetMs={100} />
              <GapBar label="Overhead" currentMs={130} targetMs={50} />
            </div>
            <div className="pa-gap-total">
              Total: ~2,100ms → 300ms target —{" "}
              <strong className="pa-bad">7x gap</strong>
            </div>
          </div>

          {/* Optimization Roadmap */}
          <div className="pa-section">
            <h4 className="pa-subtitle">Optimization Roadmap</h4>
            <table className="pa-table pa-roadmap">
              <thead>
                <tr>
                  <th>Optimization</th>
                  <th>Savings</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                <tr>
                  <td>Parallel translations</td>
                  <td>—</td>
                  <td className="pa-status-done">Done ✓</td>
                </tr>
                <tr>
                  <td>Streaming TTS playback</td>
                  <td>−500–1000ms perceived</td>
                  <td className="pa-status-medium">Medium</td>
                </tr>
                <tr>
                  <td>Shorter utterance chunks</td>
                  <td>−300–500ms</td>
                  <td className="pa-status-tradeoff">Quality tradeoff</td>
                </tr>
                <tr>
                  <td>GPU TTS (A100/T4)</td>
                  <td>−500–1500ms</td>
                  <td className="pa-status-cost">Cost increase</td>
                </tr>
                <tr>
                  <td>Co-locate all models on one GPU</td>
                  <td>−100–300ms</td>
                  <td className="pa-status-hard">Hard — infra</td>
                </tr>
                <tr>
                  <td>End-to-end speech-to-speech</td>
                  <td>paradigm shift</td>
                  <td className="pa-status-rnd">Brivva's R&D goal</td>
                </tr>
              </tbody>
            </table>
            <p className="pa-footer">
              True 300ms needs co-located models or end-to-end S2S. That's the R&D challenge.
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
