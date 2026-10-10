import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import InterfaceBoundary from './components/InterfaceBoundary';
import './styles.css';
ReactDOM.createRoot(document.getElementById('root')!).render(<React.StrictMode><InterfaceBoundary><App /></InterfaceBoundary></React.StrictMode>);
