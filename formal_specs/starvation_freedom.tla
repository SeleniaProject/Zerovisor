---- MODULE starvation_freedom ----
EXTENDS TLC, Integers, Sequences, FiniteSets

CONSTANTS
    Processes,     \* Set of processes
    Resources,     \* Set of shared resources
    MaxWaitTime    \* Maximum acceptable wait time

VARIABLES
    processState,  \* State of each process
    waitTime,      \* How long each process has been waiting
    priority,      \* Priority of each process
    lastServed,    \* Last time each process was served
    globalTime     \* Global system time

vars == <<processState, waitTime, priority, lastServed, globalTime>>

\* Process states
States == {"WAITING", "RUNNING", "COMPLETED"}

\* Priority levels
Priorities == {"LOW", "NORMAL", "HIGH"}

\* Starvation freedom - no process waits indefinitely
NoStarvation ==
    \A p \in Processes :
        processState[p] = "WAITING" => waitTime[p] <= MaxWaitTime

\* Fairness - processes are served in bounded time
BoundedWaiting ==
    \A p \in Processes :
        processState[p] = "WAITING" =>
            \E t \in Nat : t <= MaxWaitTime /\ Eventually(processState[p] = "RUNNING")

\* Priority inversion prevention
NoPriorityInversion ==
    \A p1, p2 \in Processes :
        /\ processState[p1] = "WAITING"
        /\ processState[p2] = "RUNNING"
        /\ priority[p1] > priority[p2]
        => waitTime[p1] <= MaxWaitTime

\* Aging mechanism to prevent starvation
AgingMechanism ==
    \A p \in Processes :
        waitTime[p] > MaxWaitTime / 2 =>
            priority[p] = "HIGH"

\* Initial state
Init ==
    /\ processState = [p \in Processes |-> "WAITING"]
    /\ waitTime = [p \in Processes |-> 0]
    /\ priority = [p \in Processes |-> "NORMAL"]
    /\ lastServed = [p \in Processes |-> 0]
    /\ globalTime = 0

\* Serve a waiting process
ServeProcess(process) ==
    /\ processState[process] = "WAITING"
    /\ processState' = [processState EXCEPT ![process] = "RUNNING"]
    /\ lastServed' = [lastServed EXCEPT ![process] = globalTime]
    /\ waitTime' = [waitTime EXCEPT ![process] = 0]
    /\ UNCHANGED <<priority, globalTime>>

\* Complete a running process
CompleteProcess(process) ==
    /\ processState[process] = "RUNNING"
    /\ processState' = [processState EXCEPT ![process] = "COMPLETED"]
    /\ UNCHANGED <<waitTime, priority, lastServed, globalTime>>

\* Age processes to prevent starvation
AgeProcesses ==
    /\ globalTime' = globalTime + 1
    /\ waitTime' = [p \in Processes |->
        IF processState[p] = "WAITING" THEN waitTime[p] + 1 ELSE waitTime[p]]
    /\ priority' = [p \in Processes |->
        IF waitTime'[p] > MaxWaitTime / 2 THEN "HIGH" ELSE priority[p]]
    /\ UNCHANGED <<processState, lastServed>>

\* Reset completed process
ResetProcess(process) ==
    /\ processState[process] = "COMPLETED"
    /\ processState' = [processState EXCEPT ![process] = "WAITING"]
    /\ priority' = [priority EXCEPT ![process] = "NORMAL"]
    /\ UNCHANGED <<waitTime, lastServed, globalTime>>

\* Next state relation
Next ==
    \/ \E p \in Processes : ServeProcess(p)
    \/ \E p \in Processes : CompleteProcess(p)
    \/ AgeProcesses
    \/ \E p \in Processes : ResetProcess(p)

\* Specification with fairness
Spec == Init /\ [][Next]_vars /\ WF_vars(AgeProcesses) /\ 
        \A p \in Processes : WF_vars(ServeProcess(p))

\* Type correctness
TypeOK ==
    /\ processState \in [Processes -> States]
    /\ waitTime \in [Processes -> Nat]
    /\ priority \in [Processes -> Priorities]
    /\ globalTime \in Nat

\* Safety properties
StarvationFreedom == []NoStarvation
FairScheduling == []BoundedWaiting
PriorityRespected == []NoPriorityInversion

\* Liveness properties
AllProcessesEventuallyServed ==
    \A p \in Processes :
        processState[p] = "WAITING" ~> processState[p] = "RUNNING"

ProgressGuaranteed ==
    []<>(\E p \in Processes : processState[p] = "RUNNING")

\* Aging effectiveness
AgingWorks ==
    \A p \in Processes :
        waitTime[p] > MaxWaitTime / 2 ~> priority[p] = "HIGH"

====