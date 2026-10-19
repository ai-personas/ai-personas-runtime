;; Import-free compute.wasm.v1 example. Sum decimal u32 values separated by
;; ASCII spaces, commas, tabs, CR or LF. Invalid characters/values trap.
;; The profile's 8192-byte input bound makes the sum fit safely in a u64.
(module
  (memory (export "memory") 1)
  (func (export "run") (param $input i32) (param $length i32) (result i64)
    (local $at i32) (local $end i32) (local $char i32)
    (local $number i64) (local $sum i64) (local $out i32)
    (local.set $at (local.get $input))
    (local.set $end (i32.add (local.get $input) (local.get $length)))
    (block $done
      (loop $read
        (br_if $done (i32.ge_u (local.get $at) (local.get $end)))
        (local.set $char (i32.load8_u (local.get $at)))
        (if (i32.and (i32.ge_u (local.get $char) (i32.const 48))
                     (i32.le_u (local.get $char) (i32.const 57)))
          (then
            (local.set $number (i64.add (i64.mul (local.get $number) (i64.const 10))
              (i64.extend_i32_u (i32.sub (local.get $char) (i32.const 48)))))
            (if (i64.gt_u (local.get $number) (i64.const 4294967295)) (then unreachable)))
          (else
            (if (i32.or
                  (i32.or (i32.eq (local.get $char) (i32.const 32))
                          (i32.eq (local.get $char) (i32.const 44)))
                  (i32.or (i32.eq (local.get $char) (i32.const 10))
                    (i32.or (i32.eq (local.get $char) (i32.const 13))
                            (i32.eq (local.get $char) (i32.const 9)))))
              (then
                (local.set $sum (i64.add (local.get $sum) (local.get $number)))
                (local.set $number (i64.const 0)))
              (else unreachable))))
        (local.set $at (i32.add (local.get $at) (i32.const 1)))
        (br $read)))
    (local.set $sum (i64.add (local.get $sum) (local.get $number)))
    ;; Output grows backwards below byte 16384, outside the host input region.
    (local.set $out (i32.const 16384))
    (loop $digits
      (local.set $out (i32.sub (local.get $out) (i32.const 1)))
      (i32.store8 (local.get $out)
        (i32.add (i32.wrap_i64 (i64.rem_u (local.get $sum) (i64.const 10))) (i32.const 48)))
      (local.set $sum (i64.div_u (local.get $sum) (i64.const 10)))
      (br_if $digits (i64.ne (local.get $sum) (i64.const 0))))
    (i64.or (i64.shl (i64.extend_i32_u (local.get $out)) (i64.const 32))
      (i64.extend_i32_u (i32.sub (i32.const 16384) (local.get $out))))))
