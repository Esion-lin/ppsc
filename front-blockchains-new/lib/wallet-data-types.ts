export type WalletDataJob = {
  id: string; kind: 'input' | 'balance'; environmentId: string; gateway: string; owner: string
  status: 'pending' | 'encrypting' | 'uploading' | 'registering' | 'querying' | 'opening' | 'completed' | 'failed'
  created: number; dataId?: string; amount?: string; version?: string
  executionId?: string; transactionHash?: string; error?: string
}
