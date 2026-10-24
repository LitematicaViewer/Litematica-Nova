import {Easing, interpolate} from 'remotion';
export const clamp = {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'} as const;
export const ease = Easing.bezier(0.22, 1, 0.36, 1);
export const glide = Easing.bezier(0.55, 0, 0.18, 1);
export const ramp = (f: number, start: number, end: number) => interpolate(f, [start,end], [0,1], {...clamp, easing:ease});
export const mix = (a: number,b: number,t: number) => a+(b-a)*t;
