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