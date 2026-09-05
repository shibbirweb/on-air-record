import { BrowserRouter, Navigate, Route, Routes } from 'react-router';

import { AppShell } from '@/components/AppShell';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ControlRoom } from '@/pages/ControlRoom';
import { SettingsPage } from '@/pages/SettingsPage';

export default function App() {
  return (
    <BrowserRouter>
      <TooltipProvider>
        <Routes>
          <Route element={<AppShell />}>
            <Route index element={<ControlRoom />} />
            <Route path="settings" element={<SettingsPage />} />
            {/* The backend serves index.html for any unmatched path, so a stray URL lands here. */}
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
      </TooltipProvider>
    </BrowserRouter>
  );
}
