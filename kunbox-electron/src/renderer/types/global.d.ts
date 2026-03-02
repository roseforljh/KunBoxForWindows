type API = import('@shared/tauri-api').API

declare const __APP_VERSION__: string

interface Window {
  api: API
}

declare module '*.png' {
  const src: string
  export default src
}

declare module '*.svg' {
  const src: string
  export default src
}

declare module '*.jpg' {
  const src: string
  export default src
}
