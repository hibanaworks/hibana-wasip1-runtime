(set-logic QF_BV)
(define-fun mask () (_ BitVec 64) #x0000000000004042)
(define-fun read-right () (_ BitVec 64) #x0000000000000002)
(define-fun write-right () (_ BitVec 64) #x0000000000000040)
(define-fun dir-right () (_ BitVec 64) #x0000000000004000)
(declare-const kind (_ BitVec 2))
(declare-const base (_ BitVec 64))
(declare-const inheriting (_ BitVec 64))
(define-fun required () (_ BitVec 64)
  (ite (= kind #b01) read-right
    (ite (= kind #b10) write-right
      (ite (= kind #b11) dir-right #x0000000000000000))))
(define-fun applicable () (_ BitVec 64)
  (ite (= kind #b11) #x0000000000004040 #x0000000000000042))
(define-fun admitted () Bool
  (and (not (= required #x0000000000000000)) (= (bvand base applicable) required)))
; No accepted object has an empty capability.
(push)
(assert (and admitted (= kind #b00)))
(check-sat)
(pop)
; A grant cannot add an unrequested I/O mode.
(push)
(assert (and admitted (not (= (bvand required base) required))))
(check-sat)
(pop)
; Conflicting applicable I/O modes cannot silently disappear.
(push)
(assert (and admitted (not (= (bvand base applicable) required))))
(check-sat)
(pop)
; Inheriting bits cannot make a read on a writer valid.
(push)
(assert (and (= kind #b10) (= (bvand base mask) read-right) admitted))
(check-sat)
(pop)
; Old writable-default bug: read request admitted by the writer's material.
(push)
(assert (and (= kind #b10) (= base read-right) (not admitted)))
(check-sat)
(pop)
; Old union bug: an inherited writer bit changed a current read request.
(push)
(assert (and (= base read-right) (= inheriting write-right)
             (not (= (bvor base inheriting) base))))
(check-sat)
(pop)
; Valid write remains admitted.
(push)
(assert (and (= kind #b10) (= base write-right) admitted))
(check-sat)
(pop)
