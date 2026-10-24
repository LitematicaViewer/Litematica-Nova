import {zh} from './zh-CN';
import {en} from './en-US';
export type Locale = 'zh-CN' | 'en-US';
export const copy = (locale: Locale) => locale === 'zh-CN' ? zh : en;
