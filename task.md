# Zerovisor 未実装項目チェックリスト

以下のタスクは現行コードベースで未実装、または機能が不完全な項目です。実装が完了したら `[x]` に変更してください。

## 仮想化コア
- [x] EptManager: ARM64 / RISC-V 対応と TLB 無効化最適化
- [ ] IOMMU / VT-d: フル統合とデバイスパススルー
- [ ] Live Migration: 差分コピー & 停止時間最小化
- [ ] NUMAOptimizer: 動的最適化アルゴリズム完備
- [ ] RealTimeQueue & Scheduler: 割り込み遅延 < 1µs, WCET 証明
- [ ] Cross-Architecture Live Migration: 異種 ISA (x86_64 / ARM64 / RISC-V) 間のライブマイグレーション

## セキュリティ
- [ ] QuantumCrypto: Kyber/Dilithium/SPHINCS+ 統合 API
- [ ] Formal Verification CI: Coq / TLA+ 自動実行パイプライン
- [ ] Information-flow Analysis: 機密性証明
- [ ] Homomorphic Memory Encryption: FHE protected guest memory

## デバイス仮想化
- [ ] GPU / TPU / FPGA / QPU 仮想化 (SR-IOV, MIG)
- [ ] SR-IOV NIC & Storage デバイスのパススルー
- [ ] Accelerators: RISC-V Vector, AI Engine 対応

## 高可用 & 分散
- [ ] Exascale スケールアウト (>1M コア, InfiniBand 最適化)
- [ ] BFT クラスタリング: Byzantine Fault Tolerance

## エネルギー & 環境
- [ ] EnergyManager: DVFS + Thermal + Carbon-aware スケジューリング
- [ ] Thermal-aware Scheduler 統合

## クラウドネイティブ
- [ ] Kubernetes CRI ランタイム統合
- [ ] WASM ランタイム (WASI) + コールドスタート 1 ms

## テスト & ベンチ
- [ ] パフォーマンステスト自動化 (VMEXIT < 10ns, VM 起動 < 50ms) 