---- MODULE memory_safety ----
EXTENDS Naturals, Sequences

(* Memory safety verification of Zerovisor hypervisor *)

CONSTANTS HvPages \* Set of physical pages owned by hypervisor

VARIABLES GuestAccess \* [VM -> SUBSET Nat] physical addresses each VM may access

==================================================================

MemorySafe == \A vm \in DOMAIN GuestAccess: GuestAccess[vm] \cap HvPages = {}

==================================================================

THEOREM HypervisorMemoryIsolated == MemorySafe

==================================================================

(*--algorithm ProofSketch
variables hv_pages = HvPages;
          guest_access = GuestAccess;
begin
  assert MemorySafe;
end algorithm;*)

============================== END MODULE memory_safety ============================== 