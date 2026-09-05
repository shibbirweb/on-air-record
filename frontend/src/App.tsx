import { TooltipProvider } from '@/components/ui/tooltip';
import { ControlRoom } from '@/pages/ControlRoom';

export default function App() {
  return (
    <TooltipProvider>
      <ControlRoom />
    </TooltipProvider>
  );
}
