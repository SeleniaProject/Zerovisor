# Zerovisor 未実装項目チェックリスト

以下のタスクは設計書・要件書に記載されているものの、現行コードベースでは未実装または不完全と判断された項目です。実装が完了したらチェックを入れてください。

- [x] VmxManager: VMX 制御構造プール管理と VM 起動シーケンスの実装
- [x] EptManager: EPT テーブル生成・更新・TLB 無効化の完全実装
- [x] IsolationEngine: ゲスト間メモリ・デバイス分離ポリシーの強制
- [x] ARM64 仮想化拡張 (EL2) サポート
- [x] RISC-V Hypervisor Extension サポート
- [x] GPU / TPU / FPGA / QPU 仮想化 (SR-IOV, MIG 対応)
- [x] IOMMU / VT-d 統合とデバイスパススルー
- [x] NUMAOptimizer: VM 配置・メモリ移動最適化アルゴリズム
- [x] ライブマイグレーション機能 (メモリ差分・停止時間最小化)
- [x] 高可用クラスタリング (Byzantine Fault Tolerance 含む)
- [x] エネルギー最適化 (DVFS, Thermal, カーボンアウェアスケジューリング)
- [ ] リアルタイム保証 (割り込み遅延 < 1µs, WCET 証明)
- [ ] 形式検証 (Coq / TLA+) 自動 CI 統合
- [ ] MonitoringEngine: UART + Prometheus メトリクスエクスポート
- [ ] DebugInterface: GDB スタブ・ブレークポイント・トレースポイント
- [ ] PluginManager & HypervisorPlugin フレームワーク
- [ ] FeatureRegistry: 動的機能の有効化 / 無効化機構
- [ ] Kubernetes CRI / WASM ランタイム統合
- [ ] パフォーマンステスト (VMEXIT < 10ns, VM 起動 < 50ms) の自動化
- [ ] RemoteAttestation: 外部検証サーバー連携と証明検証 