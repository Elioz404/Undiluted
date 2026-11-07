/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_NEAR_RPC_URL?: string;
  readonly VITE_ANCHOR_CONTRACT?: string;
  readonly VITE_ENCLAVE_SIGNING_KEY?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
