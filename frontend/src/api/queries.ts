import { QueryClient, useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "./client";
import type { Drive } from "./types/Drive";
import type { DrivesResponse } from "./types/DrivesResponse";
import type { Health } from "./types/Health";
import type { CategoryTotal } from "./types/CategoryTotal";
import type { EntryView } from "./types/EntryView";
import type { ExtBreakdown } from "./types/ExtBreakdown";
import type { Page } from "./types/Page";
import type { TreeResponse } from "./types/TreeResponse";
import type { JobCreated } from "./types/JobCreated";
import type { Measure } from "./types/Measure";
import type { ScanStarted } from "./types/ScanStarted";
import type { Snapshot } from "./types/Snapshot";
import type { SnapshotData } from "./types/SnapshotData";
import type { JobStatus } from "./types/JobStatus";
import type { Settings } from "./types/Settings";
import type { SystemInfo } from "./types/SystemInfo";
import type { AppsResponse } from "./types/AppsResponse";
import type { StalePageView } from "./types/StalePageView";
import type { DupGroupsPage } from "./types/DupGroupsPage";
import type { DupRun } from "./types/DupRun";
import type { NearView } from "./types/NearView";
import type { SkipRow } from "./types/SkipRow";
import type { SuggestResult } from "./types/SuggestResult";
import type { CandidatePage } from "./types/CandidatePage";
import type { CleanupOverview } from "./types/CleanupOverview";
import type { Risk } from "./types/Risk";
import type { RuleKind } from "./types/RuleKind";
import type { RulesView } from "./types/RulesView";
import type { SnapshotCleanup } from "./types/SnapshotCleanup";

export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: 1, refetchOnWindowFocus: false, staleTime: 5_000 },
    },
  });
}

export const keys = {
  health: ["health"] as const,
  system: ["system"] as const,
  settings: ["settings"] as const,
  jobs: ["jobs"] as const,
  drives: ["drives"] as const,
  drive: (id: string) => ["drives", id] as const,
  snapshots: ["snapshots"] as const,
  snapshotList: (volumeId?: string) => ["snapshots", "list", volumeId ?? "all"] as const,
  latest: ["snapshots", "latest"] as const,
  top: (id: number, kind: string, measure: Measure, under?: string) => ["snapshots", id, kind, measure, under ?? ""] as const,
};

export function useHealth() {
  return useQuery({
    queryKey: keys.health,
    queryFn: ({ signal }) => api<Health>("/health", { signal }),
    refetchInterval: 4_000,
    retry: false,
    staleTime: 0,
  });
}

export function useSystem() {
  return useQuery({ queryKey: keys.system, queryFn: ({ signal }) => api<SystemInfo>("/system", { signal }) });
}

export function useSettings() {
  return useQuery({ queryKey: keys.settings, queryFn: ({ signal }) => api<Settings>("/settings", { signal }) });
}

export function useSaveSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (s: Settings) => api<Settings>("/settings", { method: "PUT", body: s }),
    onSuccess: (saved) => {
      qc.setQueryData(keys.settings, saved);
      // Low-space flags depend on the thresholds.
      void qc.invalidateQueries({ queryKey: keys.drives });
    },
  });
}

export function useJobs() {
  return useQuery({ queryKey: keys.jobs, queryFn: ({ signal }) => api<JobStatus[]>("/jobs?limit=20", { signal }) });
}

export function useDrives() {
  return useQuery({
    queryKey: keys.drives,
    queryFn: ({ signal }) => api<DrivesResponse>("/drives", { signal }),
    staleTime: 30_000,
  });
}

export function useDrive(id: string) {
  return useQuery({ queryKey: keys.drive(id), queryFn: ({ signal }) => api<Drive>(`/drives/${id}`, { signal }) });
}

export function useRefreshDrives() {
  return useMutation({ mutationFn: () => api<JobCreated>("/drives/refresh", { method: "POST" }) });
}
export function useSnapshots(volumeId?: string) {
  const q = volumeId ? `?volume_id=${encodeURIComponent(volumeId)}&limit=50` : "?limit=50";
  return useQuery({
    queryKey: keys.snapshotList(volumeId),
    queryFn: ({ signal }) => api<Snapshot[]>(`/snapshots${q}`, { signal }),
  });
}

export function useLatestSnapshots() {
  return useQuery({ queryKey: keys.latest, queryFn: ({ signal }) => api<Snapshot[]>("/snapshots/latest", { signal }) });
}

function underParam(under?: string) {
  return under ? `&under=${encodeURIComponent(under)}` : "";
}

export function useTopFiles(id: number | undefined, measure: Measure, limit = 20, under?: string) {
  return useQuery({
    queryKey: keys.top(id ?? -1, `files-${limit}`, measure, under),
    queryFn: ({ signal }) =>
      api<SnapshotData<EntryView[]>>(`/snapshots/${id}/top-files?measure=${measure}&limit=${limit}${underParam(under)}`, { signal }),
    enabled: id !== undefined,
    staleTime: Infinity,
  });
}

export function useTopDirs(id: number | undefined, measure: Measure, limit = 20, under?: string, maxDepth?: number) {
  const depth = maxDepth ? `&max_depth=${maxDepth}` : "";
  return useQuery({
    queryKey: keys.top(id ?? -1, `dirs-${limit}-${maxDepth ?? 0}`, measure, under),
    queryFn: ({ signal }) =>
      api<SnapshotData<EntryView[]>>(
        `/snapshots/${id}/top-dirs?measure=${measure}&limit=${limit}${depth}${underParam(under)}`,
        { signal },
      ),
    enabled: id !== undefined,
    staleTime: Infinity,
  });
}

export function useCategories(id: number | undefined, under?: string) {
  return useQuery({
    queryKey: keys.top(id ?? -1, "categories", "alloc", under),
    queryFn: ({ signal }) =>
      api<SnapshotData<CategoryTotal[]>>(`/snapshots/${id}/categories?x=1${underParam(under)}`, { signal }),
    enabled: id !== undefined,
    staleTime: Infinity,
  });
}

function useInvalidateSnapshots() {
  const qc = useQueryClient();
  return () => qc.invalidateQueries({ queryKey: keys.snapshots });
}

export function useStartScan() {
  const invalidate = useInvalidateSnapshots();
  return useMutation({
    mutationFn: (roots: string[]) => api<ScanStarted>("/scans", { method: "POST", body: { roots } }),
    onSuccess: () => void invalidate(),
  });
}

export function useResumeScan() {
  const invalidate = useInvalidateSnapshots();
  return useMutation({
    mutationFn: (snapshotId: number) => api<JobCreated>(`/snapshots/${snapshotId}/resume`, { method: "POST" }),
    onSuccess: () => void invalidate(),
  });
}

export function useDeleteSnapshot() {
  const invalidate = useInvalidateSnapshots();
  return useMutation({
    mutationFn: (snapshotId: number) => api<unknown>(`/snapshots/${snapshotId}`, { method: "DELETE" }),
    onSuccess: () => void invalidate(),
  });
}

export function useCancelJob() {
  return useMutation({ mutationFn: (jobId: number) => api<JobStatus>(`/jobs/${jobId}/cancel`, { method: "POST" }) });
}
const PAGE = 20;

export function useTree(id: number | undefined, node: number, measure: Measure) {
  return useQuery({
    queryKey: ["snapshots", id, "tree", node, measure],
    queryFn: ({ signal }) =>
      api<SnapshotData<TreeResponse>>(`/snapshots/${id}/tree?node=${node}&depth=2&measure=${measure}`, { signal }),
    enabled: id !== undefined,
    staleTime: Infinity,
    placeholderData: (prev) => prev,
  });
}

/** Largest files/folders under `node`, 20 per page ("Show more"). */
export function useTopInfinite(
  id: number | undefined,
  kind: "files" | "dirs",
  node: number,
  measure: Measure,
  category?: string | null,
) {
  const cat = kind === "files" && category ? `&category=${category}` : "";
  const depth = kind === "dirs" ? "&max_depth=3" : "";
  return useInfiniteQuery({
    queryKey: ["snapshots", id, `top-${kind}`, node, measure, category ?? ""],
    queryFn: ({ pageParam, signal }) =>
      api<SnapshotData<EntryView[]>>(
        `/snapshots/${id}/top-${kind}?node=${node}&measure=${measure}&limit=${PAGE}&offset=${pageParam}${cat}${depth}`,
        { signal },
      ),
    initialPageParam: 0,
    getNextPageParam: (last, all) => (last.data.length === PAGE ? all.length * PAGE : undefined),
    enabled: id !== undefined,
    staleTime: Infinity,
  });
}

/** Children of a folder beyond the chart's cap (the "Other" bucket). */
export function useChildrenInfinite(id: number | undefined, node: number | null, measure: Measure, skip: number) {
  return useInfiniteQuery({
    queryKey: ["snapshots", id, "children", node, measure, skip],
    queryFn: ({ pageParam, signal }) =>
      api<SnapshotData<Page<EntryView>>>(
        `/snapshots/${id}/children?node=${node}&measure=${measure}&limit=${PAGE}&cursor=${pageParam}`,
        { signal },
      ),
    initialPageParam: skip,
    getNextPageParam: (last) => last.data.next_cursor ?? undefined,
    enabled: id !== undefined && node !== null,
    staleTime: Infinity,
  });
}

export function useCategoriesAt(id: number | undefined, node: number) {
  return useQuery({
    queryKey: ["snapshots", id, "categories", node],
    queryFn: ({ signal }) => api<SnapshotData<CategoryTotal[]>>(`/snapshots/${id}/categories?node=${node}`, { signal }),
    enabled: id !== undefined,
    staleTime: Infinity,
  });
}

export function useExtensionsAt(id: number | undefined, node: number) {
  return useQuery({
    queryKey: ["snapshots", id, "extensions", node],
    queryFn: ({ signal }) => api<SnapshotData<ExtBreakdown>>(`/snapshots/${id}/extensions?node=${node}&limit=20`, { signal }),
    enabled: id !== undefined,
    staleTime: Infinity,
  });
}

export function useSnapshot(id: number) {
  return useQuery({ queryKey: ["snapshots", "one", id], queryFn: ({ signal }) => api<Snapshot>(`/snapshots/${id}`, { signal }) });
}

export function useReveal() {
  return useMutation({
    mutationFn: (v: { snapshotId: number; entryId: number }) =>
      api<void>("/shell/reveal", { method: "POST", body: { snapshot_id: v.snapshotId, entry_id: v.entryId } }),
  });
}
// ---------------------------------------------------------------------------
// Cleanup suggestions and rules
// ---------------------------------------------------------------------------

export const cleanupKeys = {
  all: ["cleanup"] as const,
  overview: ["cleanup", "overview"] as const,
  snapshot: (id: number) => ["cleanup", "snapshot", id] as const,
  rules: ["cleanup", "rules"] as const,
};

export function useCleanupOverview() {
  return useQuery({
    queryKey: cleanupKeys.overview,
    queryFn: ({ signal }) => api<CleanupOverview>("/cleanup/overview", { signal }),
    staleTime: 10_000,
  });
}

export function useSnapshotCleanup(id: number | undefined) {
  return useQuery({
    queryKey: cleanupKeys.snapshot(id ?? -1),
    queryFn: ({ signal }) => api<SnapshotCleanup>(`/snapshots/${id}/cleanup`, { signal }),
    enabled: id !== undefined,
    staleTime: 10_000,
  });
}

export interface CandidateFilter {
  /** One scan; omitted = the latest scan of every drive. */
  snapshotId?: number;
  /** Drive id (volume) when no snapshot is given. */
  drive?: string;
  risk?: Risk | null;
  category?: string | null;
  kind?: RuleKind | null;
  q?: string;
}

export const CANDIDATE_PAGE = 20;

function candidateQuery(f: CandidateFilter, extra: Record<string, string | number | undefined> = {}): string {
  const p = new URLSearchParams();
  const set = (k: string, v: string | number | null | undefined) => {
    if (v !== undefined && v !== null && v !== "") p.set(k, String(v));
  };
  set("snapshot_id", f.snapshotId);
  set("drive", f.drive);
  set("risk", f.risk);
  set("category", f.category);
  set("kind", f.kind);
  set("q", f.q?.trim());
  for (const [k, v] of Object.entries(extra)) set(k, v);
  return p.toString();
}

/** Ranked cleanup candidates, 20 per page ("Show more"). */
export function useCandidates(filter: CandidateFilter) {
  return useInfiniteQuery({
    queryKey: ["cleanup", "candidates", filter] as const,
    queryFn: ({ pageParam, signal }) =>
      api<CandidatePage>(`/cleanup/candidates?${candidateQuery(filter, { limit: CANDIDATE_PAGE, cursor: pageParam })}`, { signal }),
    initialPageParam: 0,
    getNextPageParam: (last) => last.next_cursor ?? undefined,
    staleTime: 10_000,
  });
}

/** Smaller matches listed under one candidate. */
export function useNestedCandidates(snapshotId: number, parent: number, enabled: boolean) {
  return useQuery({
    queryKey: ["cleanup", "nested", snapshotId, parent] as const,
    queryFn: ({ signal }) =>
      api<CandidatePage>(`/cleanup/candidates?snapshot_id=${snapshotId}&parent=${parent}&limit=50`, { signal }),
    enabled,
    staleTime: 60_000,
  });
}

export function useRules() {
  return useQuery({
    queryKey: cleanupKeys.rules,
    queryFn: ({ signal }) => api<RulesView>("/rules", { signal }),
    staleTime: 5_000,
  });
}

/** Run the rules again (after editing rule files, or after a failed run). */
export function useRerunRules() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (snapshotIds: number[] = []) =>
      api<JobCreated>("/cleanup/evaluate", { method: "POST", body: { snapshot_ids: snapshotIds } }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: cleanupKeys.all }),
  });
}

// ---------------------------------------------------------------------------
// Stale data and installed apps
// ---------------------------------------------------------------------------

/** Files and folders not touched for `months`, largest first, 20 per page. */
export function useStale(id: number | undefined, months: number) {
  return useInfiniteQuery({
    queryKey: ["snapshots", id, "stale", months] as const,
    queryFn: ({ pageParam, signal }) =>
      api<SnapshotData<StalePageView>>(`/snapshots/${id}/stale?months=${months}&limit=20&cursor=${pageParam}`, { signal }),
    initialPageParam: 0,
    getNextPageParam: (last) => last.data.next_cursor ?? undefined,
    enabled: id !== undefined,
    staleTime: Infinity,
  });
}

/** Large files ranked by size x age. */
export function useOldLarge(id: number | undefined, floorMib: number) {
  return useInfiniteQuery({
    queryKey: ["snapshots", id, "old-large", floorMib] as const,
    queryFn: ({ pageParam, signal }) =>
      api<SnapshotData<StalePageView>>(`/snapshots/${id}/old-large?floor_mib=${floorMib}&limit=20&cursor=${pageParam}`, { signal }),
    initialPageParam: 0,
    getNextPageParam: (last) => last.data.next_cursor ?? undefined,
    enabled: id !== undefined,
    staleTime: Infinity,
  });
}

export function useApps() {
  return useQuery({
    queryKey: ["apps"] as const,
    queryFn: ({ signal }) => api<AppsResponse>("/apps", { signal }),
    staleTime: 60_000,
  });
}

// ---------------------------------------------------------------------------
// Duplicates
// ---------------------------------------------------------------------------

export interface DupFilter {
  drive?: string;
  folder?: string;
  category?: string;
  sort?: "wasted" | "size" | "count";
}

function dupQuery(f: DupFilter, extra: Record<string, string | number> = {}): string {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries({ ...f, ...extra })) if (v !== undefined && v !== "") p.set(k, String(v));
  return p.toString();
}

export function useDupStatus() {
  return useQuery({
    queryKey: ["duplicates", "status"] as const,
    queryFn: ({ signal }) => api<DupRun | null>("/duplicates/status", { signal }),
    staleTime: 5_000,
  });
}

/** Duplicate groups, 20 per page, largest wasted space first. */
export function useDupGroups(filter: DupFilter, enabled = true) {
  return useInfiniteQuery({
    queryKey: ["duplicates", "groups", filter] as const,
    queryFn: ({ pageParam, signal }) =>
      api<DupGroupsPage>(`/duplicates/groups?${dupQuery(filter, { limit: 20, cursor: pageParam })}`, { signal }),
    initialPageParam: 0,
    getNextPageParam: (last) => last.next_cursor ?? undefined,
    enabled,
    staleTime: 10_000,
  });
}

export function useDupNear(enabled: boolean) {
  return useQuery({
    queryKey: ["duplicates", "near"] as const,
    queryFn: ({ signal }) => api<NearView[]>("/duplicates/near", { signal }),
    enabled,
  });
}

export function useDupSkips(reason: string | null) {
  return useQuery({
    queryKey: ["duplicates", "skips", reason] as const,
    queryFn: ({ signal }) => api<SkipRow[]>(`/duplicates/skips?reason=${reason}`, { signal }),
    enabled: reason !== null,
  });
}

export interface DupStart {
  snapshot_ids?: number[];
  min_size_mib?: number;
  near_duplicates?: boolean;
}

export function useStartDuplicates() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (req: DupStart) => api<JobCreated>("/duplicates/runs", { method: "POST", body: req }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["duplicates"] }),
  });
}

export interface SuggestBody {
  rule: string;
  value?: string;
  group_id?: number;
  drive?: string;
  folder?: string;
  category?: string;
}

export function useSuggestKeep() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (b: SuggestBody) => api<SuggestResult>("/duplicates/suggestions", { method: "POST", body: b }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["duplicates", "groups"] }),
  });
}

export function useClearSuggestions() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api<void>("/duplicates/suggestions", { method: "DELETE" }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["duplicates", "groups"] }),
  });
}
