import { QueryClient, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "./client";
import type { Health } from "./types/Health";
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
    onSuccess: (saved) => qc.setQueryData(keys.settings, saved),
  });
}

export function useJobs() {
  return useQuery({ queryKey: keys.jobs, queryFn: ({ signal }) => api<JobStatus[]>("/jobs?limit=20", { signal }) });
}
