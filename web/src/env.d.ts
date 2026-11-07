/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_NEAR_RPC_URL?: string;
  readonly VITE_ANCHOR_CONTRACT?: string;
  readonly VITE_ENCLAVE_SIGNING_KEY?: string;
  readonly VITE_ENCLAVE_WASM_SHA256?: string;
  readonly VITE_OUTLAYER_REGISTER_CONTRACT?: string;
  readonly VITE_RELAY_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
