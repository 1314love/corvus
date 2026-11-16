/**
 * Simplified Chinese UI message catalog.
 *
 * Keep the key set in sync with en.ts; tests/contract/i18n-catalogs.test.ts
 * enforces parity.
 */

import type { MessageCatalog } from './en';

const zhCN: MessageCatalog = {
  app: {
    title: 'Corvus',
    popupTitle: 'Corvus',
    optionsTitle: 'Corvus 设置',
    version: 'Corvus {version}',
  },
  common: {
    settings: '设置',
    refresh: '刷新',
    refreshStatus: '刷新状态',
    save: '保存',
    connect: '连接',
    disconnect: '断开',
    saveAndConnect: '保存并连接',
    language: '语言',
    english: 'English',
    chinese: '中文',
    notConnected: '未连接',
    none: '无',
    unknown: '未知',
  },
  status: {
    connected: '已连接',
    connecting: '连接中',
    disconnected: '未连接',
    pairing_required: '需要配对',
    error: '错误',
    unknown: '未知',
  },
  options: {
    pairing: '本地配对',
    pairingHelp: '请输入本地服务的回环地址。默认值为 ',
    baseUrl: '本地地址',
    browser: '浏览器',
    token: '配对令牌',
    tokenPresent: '已保存',
    tokenMissing: '未保存',
    tokenSaved: '令牌已保存。请输入新令牌以替换。',
    tokenPlaceholder: '粘贴 43 个字符的配对令牌',
    status: '状态',
    service: '服务',
    lastError: '最近错误',
  },
  notice: {
    saved: '已保存。正在连接...',
    connecting: '正在连接...',
    disconnected: '已断开。',
  },
  validation: {
    base: '请输入回环 ws:// URL，例如 ws://127.0.0.1:7330。',
    token: '请输入 43 个字符的配对令牌。',
  },
};

export default zhCN;
