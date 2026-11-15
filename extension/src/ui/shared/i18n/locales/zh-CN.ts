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
    save: '保存',
    resetToDefault: '恢复默认值',
    resetToAuto: '重置为自动',
    reconnect: '重新连接',
    detectNow: '立即检测',
    refreshStatus: '刷新状态',
    daemon: '守护进程',
    browser: '浏览器',
    cdp: 'CDP',
    language: '语言',
    english: 'English',
    chinese: '中文',
    notConnected: '未连接',
    notDetected: '未检测到',
    available: '可用',
    unavailable: '不可用',
    yes: '是',
    no: '否',
    none: '无',
    unknown: '未知',
    expected: '期望',
  },
  status: {
    connected: '已连接',
    connecting: '连接中',
    disconnected: '未连接',
    degraded: '降级',
    unknown: '未知',
  },
  options: {
    daemonConnection: '守护进程连接',
    daemonHelpBefore: '离屏文档会连接到此 WebSocket URL。本地默认值为 ',
    daemonHelpAfter: '。',
    browserCdpEndpoint: '浏览器 CDP 端点',
    cdpHelpBefore: '留空或使用 ',
    cdpHelpMid: ' 探测本地端口 9222、9223 和 9333。检测只会读取 ',
    cdpHelpAfter: '；不会附加调试器。',
    endpoint: '端点',
    protocol: '协议',
    source: '来源',
    browserFamily: '浏览器家族',
    reason: '原因',
    status: '状态',
    daemonUrl: '守护进程 URL',
    browserId: '浏览器 ID',
    daemonVersion: '守护进程版本',
    capabilities: '能力',
    lastError: '最近错误',
  },
  family: {
    match: '匹配',
    mismatch: '不匹配',
    unknown: '未知',
  },
  source: {
    auto: '自动',
    configured: '已配置',
    unknown: '未知',
  },
  notice: {
    daemonSaved: '已保存。正在重新连接...',
    daemonReset: '已恢复本地默认值。正在重新连接...',
    cdpSaved: 'CDP 设置已保存。检测已刷新。',
    cdpReset: 'CDP 设置已重置为自动。',
    cdpRefreshed: 'CDP 检测已刷新。',
    reconnecting: '正在重新连接...',
  },
  errors: {
    policyDenied: '已被守护进程策略阻止',
    policyDeniedWithReason: '已被守护进程策略阻止：{reason}',
  },
  validation: {
    daemon: '请输入有效的守护进程 URL。',
    cdp: '请输入 auto 或回环 http:// URL。',
  },
};

export default zhCN;
