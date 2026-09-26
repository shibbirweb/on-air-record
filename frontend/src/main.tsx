import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import App from './App';
import { applyPreferredTheme } from './hooks/useTheme';
import './index.css';

// Before anything renders, so no screen, signed in or not, flashes or stays in the wrong theme.
applyPreferredTheme();

const container = document.getElementById('root');
if (!container) {
  throw new Error('the root element is missing from index.html');
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
