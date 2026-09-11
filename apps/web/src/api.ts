// anole-server REST 客户端。字段形状与 crates/server 现有 REST 路由的
// snake_case 风格一致（新 web 路由统一 snake_case）。

const API_BASE: string = import.meta.env.VITE_API_BASE ?? "";

export type ApiErrorBody = {
  code?: string;
  stage?: string;
  message: string;
  action?: string;
  retryable?: boolean;
  diagnostic?: string | null;
};

export class ApiRequestError extends Error implements ApiErrorBody {
  code?: string;
  stage?: string;
  action?: string;
  retryable?: boolean;
  diagnostic?: string | null;

  constructor(body: ApiErrorBody) {
    super(body.message);
    this.name = "ApiRequestError";
    this.code = body.code;
    this.stage = body.stage;
    this.action = body.action;
    this.retryable = body.retryable;
    this.diagnostic = body.diagnostic;
  }
}

async function parseError(response: Response): Promise<ApiRequestError> {
  let body: ApiErrorBody = { message: `${response.status} ${response.statusText}` };
  try {
    const parsed = (await response.json()) as ApiErrorBody;
    if (typeof parsed?.message === "string") body = parsed;
  } catch {
    // Non-JSON error bodies keep the status-line fallback.
  }
  return new ApiRequestError(body);
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, init);
  if (!response.ok) throw await parseError(response);
  return (await response.json()) as T;
}

export type UploadTicket = {
  upload_id: string;
  file_name: string;
  size_bytes: number;
  expires_at: number;
  ttl_secs: number;
  max_upload_bytes: number;
};

export type RouteAvailabilityView = {
  target_format: string;
  available: boolean;
  required_engines: string[];
  missing_engines: string[];
  message: string;
};

export type CapabilitySnapshotView = {
  input_extension?: string | null;
  routes: Record<string, RouteAvailabilityView>;
};

export type PlanStepView = {
  step_id: string;
  capability_id: string;
  engine: { engine_id: string; engine_version?: string };
  operation: string;
  loss_class: string;
};

export type PlanView = {
  plan_id: string;
  plan_hash: string;
  target_format: string;
  steps: PlanStepView[];
  changes: { preserved: string[]; changed: string[]; dropped: string[]; unknown: string[] };
  estimated_output_bytes?: number | null;
};

export type PlanPreview = {
  probe: { format: { id: string }; artifact?: { display_path?: string | null } };
  plan: PlanView;
  plan_hash: string;
};

export type ValidationCheckView = {
  code: string;
  status: string;
  required: boolean;
  message: string;
};

export type ValidationReportView = {
  plan_hash: string;
  status: string;
  checks: ValidationCheckView[];
  intentional_changes?: string[];
};

export type WebJobView = {
  job_id: string;
  upload_id: string;
  state: "queued" | "running" | "succeeded" | "failed";
  target_format: string;
  created_at: number;
  expires_at: number;
  download_url: string | null;
  download_name: string | null;
  is_directory_output: boolean;
  validation: ValidationReportView | null;
  error: ApiErrorBody | null;
};

export async function uploadFile(file: File): Promise<UploadTicket> {
  const form = new FormData();
  form.append("file", file, file.name);
  return request<UploadTicket>("/v1/uploads", { method: "POST", body: form });
}

export async function fetchCapabilities(uploadId: string): Promise<CapabilitySnapshotView> {
  return request<CapabilitySnapshotView>(`/v1/uploads/${uploadId}/capabilities`);
}

export async function fetchPlan(uploadId: string, target: string): Promise<PlanPreview> {
  return request<PlanPreview>(`/v1/uploads/${uploadId}/plan`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ target_format: target }),
  });
}

export async function createJob(uploadId: string, target: string): Promise<{ job_id: string; state: string; poll_url: string }> {
  return request<{ job_id: string; state: string; poll_url: string }>("/v1/jobs", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ upload_id: uploadId, target_format: target }),
  });
}

export async function fetchJob(jobId: string): Promise<WebJobView> {
  return request<WebJobView>(`/v1/jobs/${jobId}`);
}

export function downloadUrl(job: WebJobView): string {
  return `${API_BASE}${job.download_url ?? ""}`;
}
