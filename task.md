# Zerovisor 未実装項目チェックリスト

以下のタスクは現行コードベースで未実装、または機能が不完全な項目です。実装が完了したら `[x]` に変更してください。

## 仮想化コア
- [x] EptManager: ARM64 / RISC-V 対応と TLB 無効化最適化
- [x] IOMMU / VT-d: フル統合とデバイスパススルー
- [x] Live Migration: 差分コピー & 停止時間最小化
- [x] NUMAOptimizer: 動的最適化アルゴリズム完備
- [x] RealTimeQueue & Scheduler: 割り込み遅延 < 1µs, WCET 証明

## セキュリティ
- [x] QuantumCrypto: Kyber/Dilithium/SPHINCS+ 統合 API
- [x] Formal Verification CI: Coq / TLA+ 自動実行パイプライン
- [x] Information-flow Analysis: 機密性証明

## デバイス仮想化
- [x] GPU / TPU / FPGA / QPU 仮想化 (SR-IOV, MIG)
- [x] SR-IOV NIC & Storage デバイスのパススルー
- [x] Accelerators: RISC-V Vector, AI Engine 対応

## 高可用 & 分散
- [x] Exascale スケールアウト (>1M コア, InfiniBand 最適化)

## エネルギー & 環境
- [x] EnergyManager: DVFS + Thermal + Carbon-aware スケジューリング
- [x] Thermal-aware Scheduler 統合

## クラウドネイティブ
- [x] Kubernetes CRI ランタイム統合
- [x] WASM ランタイム (WASI) + コールドスタート 1 ms

## テスト & ベンチ
- [x] パフォーマンステスト自動化 (VMEXIT < 10ns, VM 起動 < 50ms) 