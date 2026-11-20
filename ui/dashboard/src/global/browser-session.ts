import { useEffect } from 'react';
import type { SocketPort } from '../session';

export function browserSocket(url: string): SocketPort {
  const socket = new WebSocket(url);
  const port: SocketPort = {
    get readyState() { return socket.readyState; },
    onopen: null, onmessage: null, onerror: null, onclose: null,
    send: text => socket.send(text), close: () => socket.close(),
  };
  socket.onopen = () => port.onopen?.();
  socket.onmessage = event => port.onmessage?.({ data: event.data });
  socket.onerror = () => port.onerror?.();
  socket.onclose = () => port.onclose?.();
  return port;
}

export function useSessionLifecycle(disconnect: () => void) {
  useEffect(() => {
    const leave = () => disconnect();
    const restore = (event: PageTransitionEvent) => { if (event.persisted) leave(); };
    window.addEventListener('pagehide', leave);
    window.addEventListener('pageshow', restore);
    return () => {
      window.removeEventListener('pagehide', leave);
      window.removeEventListener('pageshow', restore);
      disconnect();
    };
  }, [disconnect]);
}
