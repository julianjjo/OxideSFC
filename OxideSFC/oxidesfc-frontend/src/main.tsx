import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
// Tokens first: index.css consumes the custom properties this file declares.
// (postcss-import is not in the PostCSS pipeline, so this ordering has to be
// expressed here rather than as an @import inside index.css.)
import './styles/tokens.css';
import './styles/index.css';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
