import '@fontsource-variable/geist';
import React from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import './styles.scss';
import './arc-bridge.scss';

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
