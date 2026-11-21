import checksums from 'electron/checksums.json' with { type: 'json' }

// 校验缓存时使用 Electron 自带的 SHA-256，避免每次打包都联网下载校验清单。
export default { electronDownload: { checksums } }
