const listeners = new Set();

export const state = {
	page: 'home',
	module: null,
	runtime: null,
	settings: null,
	stats: null,
	loading: false,
	error: null,
};

export function patch(next) {
	Object.assign(state, next);
	for (const listener of listeners) listener(state);
}

export function subscribe(listener) {
	listeners.add(listener);
	return () => listeners.delete(listener);
}
