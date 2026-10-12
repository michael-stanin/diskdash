import { useState, type ReactNode } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { AlertsProvider } from "@/app/AlertArea";
import { EventsProvider } from "@/api/events";
import { createQueryClient } from "@/api/queries";
import { ScanDialogProvider } from "@/components/scans/ScanDialog";
import { ToastProvider } from "@/components/Toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ThemeProvider } from "@/lib/theme";

export function Providers({ children }: { children: ReactNode }) {
  const [client] = useState(createQueryClient);
  return (
    <ThemeProvider>
      <QueryClientProvider client={client}>
        <EventsProvider>
          <TooltipProvider>
            <ToastProvider>
              <ScanDialogProvider>
                <AlertsProvider>{children}</AlertsProvider>
              </ScanDialogProvider>
            </ToastProvider>
          </TooltipProvider>
        </EventsProvider>
      </QueryClientProvider>
    </ThemeProvider>
  );
}
