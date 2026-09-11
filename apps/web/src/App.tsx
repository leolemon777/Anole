import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  ApiRequestError,
  createJob,
  downloadUrl,
  fetchCapabilities,
  fetchJob,
  fetchPlan,
  uploadFile,
  type ApiErrorBody,
  type CapabilitySnapshotView,
  type PlanPreview,
  type WebJobView,
} from "./api";
import { messages, type Copy, type Language } from "./i18n";
import {
  formatBytes,
  plainLossSummary,
  recommendedTargets,
  targetOptionViews,
  type PlainLossBadge,
} from "./model";

type Phase = "pick" | "uploading" | "ready" | "submitting" | "waiting";

const POLL_INTERVAL_MS = 1200;

function parseFlowError(reason: unknown): ApiErrorBody {
  if (reason instanceof ApiRequestError) {
    return {
      code: reason.code,
      stage: reason.stage,
      message: reason.message,
      action: reason.action,
      retryable: reason.retryable,
    };
  }
  return { message: reason instanceof Error ? reason.message : String(reason) };
}

function errorTitleFor(code: string | undefined, copy: Copy): string {
  switch ((code ?? "").toUpperCase()) {
    case "UNSUPPORTED":
      return `${copy.errorTitle} · ${copy.targetUnavailableUnsupported}`;
    case "ENGINE_MISSING":
      return `${copy.errorTitle} · ${copy.targetUnavailableMissing}`;
    default:
      return copy.errorTitle;
  }
}

function lossSummaryLabel(loss: PlainLossBadge, copy: Copy): string {
  switch (loss) {
    case "lossy":
      return copy.lossySummary;
    case "drop-tracks":
      return copy.dropTracksSummary;
    case "unknown":
      return copy.unknownLossSummary;
    case "container":
      return copy.containerSummary;
    default:
      return copy.losslessSummary;
  }
}

function reportStatusLabel(status: string, copy: Copy): string {
  switch (status) {
    case "pass":
      return copy.reportStatusPass;
    case "warning":
      return copy.reportStatusWarning;
    case "fail":
      return copy.reportStatusFail;
    default:
      return copy.reportStatusUnknown;
  }
}

export default function App() {
  const [language, setLanguage] = useState<Language>(() =>
    navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en",
  );
  const copy = messages[language];

  const [file, setFile] = useState<File | null>(null);
  const [phase, setPhase] = useState<Phase>("pick");
  const [uploadId, setUploadId] = useState<string | null>(null);
  const [expiresAt, setExpiresAt] = useState<number | null>(null);
  const [capabilities, setCapabilities] = useState<CapabilitySnapshotView | null>(null);
  const [capabilitiesError, setCapabilitiesError] = useState<string | null>(null);

  // UX 铁律（Leo 2026-09-10）：上传后绝不自动锁定输出格式；下拉始终以
  // “请选择输出格式…”占位，用户主动选择后才能提交。
  const [target, setTarget] = useState("");
  const [plan, setPlan] = useState<PlanPreview | null>(null);
  const [planBusy, setPlanBusy] = useState(false);

  const [job, setJob] = useState<WebJobView | null>(null);
  const [error, setError] = useState<ApiErrorBody | null>(null);
  const [reportOpen, setReportOpen] = useState(false);
  const [dragging, setDragging] = useState(false);
  const fileInput = useRef<HTMLInputElement | null>(null);
  const uploadSequence = useRef(0);

  const recommendations = useMemo(
    () => (file ? new Set(recommendedTargets(file.name)) : new Set<string>()),
    [file],
  );

  const options = useMemo(
    () =>
      targetOptionViews(file ? [...recommendations] : [], capabilities?.routes ?? null, {
        missing: copy.targetUnavailableMissing,
        unsupported: copy.targetUnavailableUnsupported,
      }),
    [file, recommendations, capabilities, copy],
  );

  const resetFlow = useCallback(() => {
    uploadSequence.current += 1;
    setFile(null);
    setPhase("pick");
    setUploadId(null);
    setExpiresAt(null);
    setCapabilities(null);
    setCapabilitiesError(null);
    setTarget("");
    setPlan(null);
    setPlanBusy(false);
    setJob(null);
    setError(null);
    setReportOpen(false);
  }, []);

  const handleFile = useCallback(
    async (chosen: File) => {
      uploadSequence.current += 1;
      const sequence = uploadSequence.current;
      setFile(chosen);
      setPhase("uploading");
      // 新文件必须重选输出格式（不沿用上一个文件的选择）。
      setTarget("");
      setPlan(null);
      setJob(null);
      setError(null);
      setCapabilities(null);
      setCapabilitiesError(null);
      setReportOpen(false);
      try {
        const ticket = await uploadFile(chosen);
        if (uploadSequence.current !== sequence) return;
        setUploadId(ticket.upload_id);
        setExpiresAt(ticket.expires_at);
        setPhase("ready");
        try {
          const snapshot = await fetchCapabilities(ticket.upload_id);
          if (uploadSequence.current !== sequence) return;
          setCapabilities(snapshot);
        } catch {
          if (uploadSequence.current !== sequence) return;
          setCapabilitiesError(copy.capabilitiesFailed);
        }
      } catch (reason) {
        if (uploadSequence.current !== sequence) return;
        setError(parseFlowError(reason));
        setPhase("pick");
      }
    },
    [copy],
  );

  const handleTargetChange = useCallback(
    async (value: string) => {
      setTarget(value);
      setJob(null);
      setError(null);
      setReportOpen(false);
      setPlan(null);
      if (!uploadId || !value) return;
      const sequence = uploadSequence.current;
      setPlanBusy(true);
      try {
        const preview = await fetchPlan(uploadId, value);
        if (uploadSequence.current !== sequence) return;
        setPlan(preview);
      } catch (reason) {
        if (uploadSequence.current !== sequence) return;
        setError(parseFlowError(reason));
      } finally {
        if (uploadSequence.current === sequence) setPlanBusy(false);
      }
    },
    [uploadId],
  );

  const submit = useCallback(async () => {
    if (!uploadId || !target || !plan) return;
    setPhase("submitting");
    setError(null);
    try {
      const created = await createJob(uploadId, target);
      setPhase("waiting");
      setJob({
        job_id: created.job_id,
        upload_id: uploadId,
        state: "queued",
        target_format: target,
        created_at: Math.floor(Date.now() / 1000),
        expires_at: expiresAt ?? Math.floor(Date.now() / 1000),
        download_url: null,
        download_name: null,
        is_directory_output: false,
        validation: null,
        error: null,
      });
    } catch (reason) {
      setError(parseFlowError(reason));
      setPhase("ready");
    }
  }, [uploadId, target, plan, expiresAt]);

  // 轮询直到终态。
  useEffect(() => {
    if (!job || (job.state !== "queued" && job.state !== "running")) return;
    let cancelled = false;
    const timer = window.setInterval(async () => {
      try {
        const next = await fetchJob(job.job_id);
        if (cancelled) return;
        setJob(next);
        if (next.state === "failed" && next.error) setError(next.error);
      } catch (reason) {
        if (cancelled) return;
        setError(parseFlowError(reason));
      }
    }, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [job]);

  const busy = phase === "uploading" || phase === "submitting" || phase === "waiting";
  const jobActive = job?.state === "queued" || job?.state === "running";
  const succeeded = job?.state === "succeeded";
  const loss = plan ? plainLossSummary(plan.plan) : null;

  return (
    <div className="page">
      <header className="masthead">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">
            🦎
          </span>
          <div>
            <h1>{copy.product}</h1>
            <p className="tagline">{copy.tagline}</p>
          </div>
        </div>
        <label className="language">
          {copy.languageLabel}
          <select
            value={language}
            onChange={(event) => setLanguage(event.target.value as Language)}
          >
            <option value="zh-CN">中文</option>
            <option value="en">English</option>
          </select>
        </label>
      </header>

      <section className="privacy-card" aria-label={copy.privacyTitle}>
        <strong>{copy.privacyTitle}</strong>
        <p>{copy.privacyBody}</p>
        <ul className="privacy-facts">
          <li>{copy.privacyLimit}</li>
          <li>{copy.privacyConcurrency}</li>
          <li>{copy.privacyTimeout}</li>
        </ul>
      </section>

      <main className="converter-card">
        {phase === "pick" || !file ? (
          <div
            className={`dropzone${dragging ? " dropzone-active" : ""}`}
            onDragOver={(event) => {
              event.preventDefault();
              setDragging(true);
            }}
            onDragLeave={() => setDragging(false)}
            onDrop={(event) => {
              event.preventDefault();
              setDragging(false);
              const dropped = event.dataTransfer.files?.[0];
              if (dropped) void handleFile(dropped);
            }}
          >
            <h2>{copy.dropTitle}</h2>
            <p>{copy.dropBody}</p>
            <button type="button" className="primary" onClick={() => fileInput.current?.click()}>
              {copy.chooseFile}
            </button>
            <input
              ref={fileInput}
              type="file"
              hidden
              onChange={(event) => {
                const chosen = event.target.files?.[0];
                if (chosen) void handleFile(chosen);
                event.target.value = "";
              }}
            />
          </div>
        ) : (
          <section className="file-summary">
            <div className="file-line">
              <span className="file-name">{file.name}</span>
              <span className="file-size">{formatBytes(file.size)}</span>
            </div>
            {expiresAt != null && (
              <div className="file-meta">
                {copy.uploadExpires}{" "}
                <time>{new Date(expiresAt * 1000).toLocaleTimeString()}</time>
              </div>
            )}
            {!busy && !jobActive && (
              <button type="button" className="ghost" onClick={resetFlow}>
                {copy.replaceFile}
              </button>
            )}
          </section>
        )}

        {phase === "uploading" && (
          <p className="status-line">
            <span className="spinner" aria-hidden="true" />
            {copy.uploading}
          </p>
        )}

        {uploadId && !jobActive && !succeeded && (
          <>
            {capabilitiesError && <p className="notice notice-warn">{capabilitiesError}</p>}
            {!capabilities && !capabilitiesError && (
              <p className="status-line">
                <span className="spinner" aria-hidden="true" />
                {copy.checkingCapabilities}
              </p>
            )}
            {capabilities && options.length === 0 && (
              <p className="notice notice-error">{copy.noRoutes}</p>
            )}
            {capabilities && options.length > 0 && (
              <label className="target-row">
                <span>{copy.target}</span>
                {/* 用户主动选择输出格式前保持占位、不可提交（UX 铁律）。 */}
                <select
                  value={target}
                  onChange={(event) => void handleTargetChange(event.target.value)}
                >
                  <option value="" disabled hidden>
                    {copy.targetPlaceholder}
                  </option>
                  {options.map((option) => (
                    <option key={option.value} value={option.value} disabled={option.disabled}>
                      {recommendations.has(option.value)
                        ? `${option.label} · ${copy.targetRecommended}`
                        : option.label}
                    </option>
                  ))}
                </select>
              </label>
            )}

            {planBusy && (
              <p className="status-line">
                <span className="spinner" aria-hidden="true" />
                {copy.previewingPlan}
              </p>
            )}

            {plan && !planBusy && (
              <section className="plan-card" aria-label={copy.planTitle}>
                <header className="plan-head">
                  <h3>{copy.planTitle}</h3>
                  {loss && <span className={`badge badge-loss-${loss}`}>{lossSummaryLabel(loss, copy)}</span>}
                </header>
                <dl className="plan-facts">
                  <div>
                    <dt>{copy.planEngines}</dt>
                    <dd>{plan.plan.steps.map((step) => step.engine.engine_id).join(" → ")}</dd>
                  </div>
                  <div>
                    <dt>{copy.planSteps}</dt>
                    <dd>{plan.plan.steps.length}</dd>
                  </div>
                  <div>
                    <dt>{copy.planChanges}</dt>
                    <dd>
                      {copy.preserved} {plan.plan.changes.preserved.length} · {copy.changed}{" "}
                      {plan.plan.changes.changed.length} · {copy.dropped}{" "}
                      {plan.plan.changes.dropped.length} · {copy.unknown}{" "}
                      {plan.plan.changes.unknown.length}
                    </dd>
                  </div>
                  <div>
                    <dt>{copy.planHash}</dt>
                    <dd className="mono">{plan.plan_hash.slice(0, 16)}…</dd>
                  </div>
                </dl>
                <button
                  type="button"
                  className="primary"
                  disabled={!target || phase === "submitting"}
                  onClick={() => void submit()}
                >
                  {phase === "submitting" ? copy.submitting : copy.run}
                </button>
              </section>
            )}
          </>
        )}

        {job && (jobActive || succeeded) && (
          <section className="job-card" aria-live="polite">
            {jobActive && (
              <p className="status-line">
                <span className="spinner" aria-hidden="true" />
                {job.state === "queued" ? copy.jobQueued : copy.jobRunning}
              </p>
            )}
            {succeeded && (
              <>
                <h3>{copy.jobSucceeded}</h3>
                <a
                  className="button-link primary"
                  href={downloadUrl(job)}
                  download={job.download_name ?? undefined}
                >
                  {copy.downloadResult}
                  {job.download_name ? `（${job.download_name}）` : ""}
                </a>
                {job.is_directory_output && <p className="notice">{copy.directoryOutputNote}</p>}
              </>
            )}
          </section>
        )}

        {job?.state === "failed" && (
          <section className="job-card job-failed">
            <h3>{copy.jobFailed}</h3>
          </section>
        )}

        {succeeded && job?.validation && (
          <section className="report-card">
            <header className="report-head">
              <h3>{copy.reportTitle}</h3>
              <span className={`badge badge-report-${job.validation.status}`}>
                {reportStatusLabel(job.validation.status, copy)}
              </span>
            </header>
            <p className="report-meta">
              {copy.reportExpires}{" "}
              <time>{new Date(job.expires_at * 1000).toLocaleTimeString()}</time>
            </p>
            <button
              type="button"
              className="ghost"
              onClick={() => setReportOpen((open) => !open)}
            >
              {reportOpen ? copy.reportCollapse : copy.reportToggle}
            </button>
            {reportOpen && (
              <ul className="report-checks">
                {job.validation.checks.map((check) => (
                  <li key={check.code} className={`check check-${check.status}`}>
                    <span className="check-code">{check.code}</span>
                    <span className="check-message">{check.message}</span>
                  </li>
                ))}
              </ul>
            )}
          </section>
        )}

        {error && (
          <section className="error-card" role="alert">
            <h3>{errorTitleFor(error.code, copy)}</h3>
            <p>{error.message}</p>
            {error.action && (
              <p className="error-action">
                {copy.errorAction}：{error.action}
              </p>
            )}
            {!busy && (
              <button type="button" className="ghost" onClick={resetFlow}>
                {copy.convertAnother}
              </button>
            )}
          </section>
        )}

        {succeeded && (
          <button type="button" className="ghost" onClick={resetFlow}>
            {copy.convertAnother}
          </button>
        )}
      </main>

      <footer className="footer">
        <span>{copy.footerLocal}</span>
        <span>·</span>
        <a href="/openapi.json" target="_blank" rel="noreferrer">
          {copy.footerDocs}
        </a>
      </footer>
    </div>
  );
}
