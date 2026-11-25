// EC1 Rev A | removable cold inspection rest | all lengths in mm
// Open in OpenSCAD, F6 Render, then File > Export > Export as STL.
// Origin = nominal board centre; the flat bed is Z=0. No external libraries.
// Tune the dimensions below, not the STL. Recheck travel and lead clearance after edits.
board_x = 54;
board_y = 34;
clearance = 0.5;   // nominal gap PER SIDE between square board and stop
bearing = 3.4;     // depth inboard of each nominal short edge
seat_z = 8.0;      // underside of seated board above bed/table
lip = 1.2;         // shallow straight stop above underside, no retaining hook
wall = 3.0;        // stock outside the seat opening
clip = 1.6;        // 45-degree cut on each exterior corner in plan
spine_w = 10;      // low central tie, leaving both long sides open
spine_h = 1.6;

sx = board_x/2 + clearance; sy = board_y/2 + clearance;
ox = sx + wall; oy = sy + wall; inner = board_x/2 - bearing;
assert(inner>0 && seat_z>spine_h && clip<wall && spine_w<board_y);

// Right-hand end footprint. Mirroring gives the identical opposite saddle.
module end_plan() {
    polygon([[inner,-oy],[ox-clip,-oy],[ox,-oy+clip],
             [ox,oy-clip],[ox-clip,oy],[inner,oy]]);
}
module saddle() {
    // Cut the rectangular seat from the top only. The two wings and end
    // fence stand on solid material, never cantilevering above the bed.
    difference() {
        linear_extrude(height=seat_z+lip) end_plan();
        translate([-sx,-sy,seat_z]) cube([2*sx,2*sy,lip+1]);
    }
}
union() {
    saddle(); mirror([1,0,0]) saddle();
    // Small overlap at each end fuses the tie into the full-height saddles.
    translate([-inner-0.5,-spine_w/2,0]) cube([2*inner+1,spine_w,spine_h]);
}
