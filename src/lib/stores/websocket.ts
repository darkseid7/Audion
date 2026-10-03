import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { authState, triggerSync, isLoggedIn } from '$lib/stores/sync';
import { appSettings } from '$lib/stores/settings';

export interface RemoteDevice {
    deviceId: string;
    deviceName: string;
    lastSeen: number;
    playerState?: RemotePlayerState;
}

export interface RemotePlayerState {
    track: {
        id: string;
        title: string;
        artist: string;
        album: string;
        coverUrl: string;
    } | null;
    isPlaying: boolean;
    currentTime: number;
    duration: number;
    volume: number;
    shuffle: boolean;
    repeat: 'none' | 'one' | 'all';
}

export const activeRemoteDevice = writable<string | null>(null);

const INITIAL_RECONNECT_DELAY = 1000;
const MAX_RECONNECT_DELAY = 30000;

function createWebsocketStore() {
    const { subscribe, set, update } = writable<{
        connected: boolean;
        devices: RemoteDevice[];
        statusText: string;
    }>({
        connected: false,
        devices: [],
        statusText: 'Connecting...'
    });

    let socket: WebSocket | null = null;
    let reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
    let reconnectDelay = INITIAL_RECONNECT_DELAY;
    let deviceId: string | null = null;
    let activeOwner: symbol | undefined;
    let connectionGeneration = 0;
    let connecting = false;
    let disposeLifetime: (() => void) | undefined;

    function ownsConnection(generation: number): boolean {
        return activeOwner !== undefined && generation === connectionGeneration;
    }

    async function connect() {
        if (!activeOwner || socket || connecting || !get(isLoggedIn) || !get(appSettings).remoteControlEnabled) return;
        const generation = connectionGeneration;
        connecting = true;

        try {
            update(s => ({ ...s, statusText: 'Authenticating...' }));
            const serverUrl = await invoke<string>('sync_get_server_url');
            if (!ownsConnection(generation)) return;
            const token = await invoke<string | null>('sync_get_access_token');
            if (!ownsConnection(generation)) return;
            const authenticatedDeviceId = await invoke<string>('sync_get_device_id');
            if (!ownsConnection(generation)) return;
            deviceId = authenticatedDeviceId;

            if (!token) {
                console.log('[WS] Cannot connect: No access token available');
                update(s => ({ ...s, statusText: 'No access token available' }));
                scheduleReconnect(generation);
                return;
            }

            const wsUrl = serverUrl.replace(/^http/, 'ws') + `?token=${token}`;
            console.log(`[WS] Connecting to ${wsUrl.substring(0, 50)}...`);
            update(s => ({ ...s, statusText: 'Establishing real-time connection...' }));
            if (!ownsConnection(generation)) return;
            const ownedSocket = new WebSocket(wsUrl);
            socket = ownedSocket;
            const ownsSocket = () => ownsConnection(generation) && socket === ownedSocket;

            ownedSocket.onopen = () => {
                if (!ownsSocket()) return;
                console.log('[WS] Connected successfully');
                update(s => ({ ...s, connected: true, statusText: 'Real-time sync active' }));
                reconnectDelay = INITIAL_RECONNECT_DELAY;

                let deviceName = "Unknown Device";
                if (typeof window !== 'undefined') {
                    const isMobileDev = /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent);
                    deviceName = isMobileDev ? "Mobile Player" : "Desktop Player";
                }
                if (ownsSocket()) send('identify', { deviceId, deviceName });
            };

            ownedSocket.onmessage = (event) => {
                if (!ownsSocket()) return;
                try {
                    const message = JSON.parse(event.data);
                    handleMessage(message);
                } catch (err) {
                    console.error('[WS] Failed to parse message:', err);
                }
            };

            ownedSocket.onclose = (event) => {
                if (!ownsSocket()) return;
                const { wasClean, code, reason } = event;
                console.log(`[WS] Closed. Clean: ${wasClean}, Code: ${code}, Reason: ${reason}`);
                socket = null;
                update(s => ({ ...s, connected: false, statusText: 'Connection closed' }));
                scheduleReconnect(generation);
            };

            ownedSocket.onerror = (err) => {
                if (!ownsSocket()) return;
                console.error('[WS] Connection error event:', err);
            };
        } catch (err) {
            if (!ownsConnection(generation)) return;
            console.error('[WS] Connection failed:', err);
            update(s => ({ ...s, statusText: `Connection failed: ${err}` }));
            scheduleReconnect(generation);
        } finally {
            if (ownsConnection(generation)) connecting = false;
        }
    }

    const messageHandlers: Set<(type: string, payload: any) => void> = new Set();

    function handleMessage(message: any) {
        const { type, payload } = message;

        switch (type) {
            case 'sync_notify':
                console.log('[WS] Sync notification received');
                triggerSync(false);
                break;
            
            case 'devices_update':
                update(s => {
                    // Filter out own device and ensure unique IDs
                    const uniqueDevices = payload.devices.reduce((acc: RemoteDevice[], current: RemoteDevice) => {
                        if (current.deviceId === deviceId) return acc;
                        if (!acc.find(item => item.deviceId === current.deviceId)) {
                            acc.push(current);
                        }
                        return acc;
                    }, []);
                    
                    return { ...s, devices: uniqueDevices };
                });
                break;

            case 'player_state':
                update(s => {
                    const devices = s.devices.map(d => {
                        if (d.deviceId === payload.deviceId) {
                            return { ...d, playerState: payload };
                        }
                        return d;
                    });
                    return { ...s, devices };
                });
                break;
            
            case 'pong':
                // Heartbeat handled by browser automatically
                break;
        }

        // Notify registered handlers
        for (const handler of messageHandlers) {
            handler(type, payload);
        }
    }

    function onMessage(handler: (type: string, payload: any) => void) {
        messageHandlers.add(handler);
        return () => messageHandlers.delete(handler);
    }

    function scheduleReconnect(generation: number) {
        if (!ownsConnection(generation) || !get(isLoggedIn) || !get(appSettings).remoteControlEnabled) return;
        if (reconnectTimeout) clearTimeout(reconnectTimeout);
        reconnectTimeout = setTimeout(() => {
            if (!ownsConnection(generation)) return;
            reconnectTimeout = null;
            console.log(`[WS] Attempting reconnect in ${reconnectDelay}ms...`);
            void connect();
            reconnectDelay = Math.min(reconnectDelay * 2, MAX_RECONNECT_DELAY);
        }, reconnectDelay);
        update(s => ({ ...s, statusText: `Reconnecting in ${Math.ceil(reconnectDelay/1000)}s...` }));
    }

    function send(type: string, payload: any) {
        if (socket && socket.readyState === WebSocket.OPEN) {
            socket.send(JSON.stringify({ type, payload }));
        }
    }

    function disconnect() {
        // Invalidate pending authentication and queued callbacks before close().
        connectionGeneration++;
        connecting = false;
        if (reconnectTimeout) clearTimeout(reconnectTimeout);
        reconnectTimeout = null;
        const previousSocket = socket;
        socket = null;
        if (previousSocket) {
            previousSocket.onopen = null;
            previousSocket.onmessage = null;
            previousSocket.onclose = null;
            previousSocket.onerror = null;
            previousSocket.close();
        }
        set({ connected: false, devices: [], statusText: 'Disconnected' });
    }

    // Auto connect/disconnect only while a desktop lifetime owns the store.
    function initialize() {
        disposeLifetime?.();
        const owner = Symbol('websocket-lifetime');
        activeOwner = owner;
        const stopAuth = authState.subscribe($auth => {
            if (activeOwner !== owner) return;
            if ($auth.is_logged_in && get(appSettings).remoteControlEnabled) {
                void connect();
            } else {
                disconnect();
            }
        });
        const stopSettings = appSettings.subscribe($settings => {
            if (activeOwner !== owner) return;
            if ($settings.remoteControlEnabled && get(authState).is_logged_in) {
                void connect();
            } else if (!$settings.remoteControlEnabled) {
                disconnect();
            }
        });
        let disposed = false;
        const dispose = () => {
            if (disposed) return;
            disposed = true;
            stopAuth();
            stopSettings();
            if (activeOwner !== owner) return;
            activeOwner = undefined;
            disposeLifetime = undefined;
            disconnect();
        };
        disposeLifetime = dispose;
        return dispose;
    }

    return {
        initialize,
        subscribe,
        send,
        connect,
        disconnect,
        onMessage,
        getDeviceId: () => deviceId
    };
}

export const wsStore = createWebsocketStore();
