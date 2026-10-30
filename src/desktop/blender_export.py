"""Trusted Blender preview exporter. Executed with --disable-autoexec; never runs file scripts."""
import bpy, sys
args=sys.argv[sys.argv.index("--")+1:]
if len(args)!=2 or args[0]!="--output": raise SystemExit("expected --output PATH")
out=args[1]
# Export evaluated scene geometry to a portable GLB. Do not run user scripts or macros.
result=bpy.ops.export_scene.gltf(filepath=out,export_format="GLB",export_apply=True,
    export_animations=False,export_cameras=False,export_lights=False)
if "FINISHED" not in result: raise SystemExit("Blender GLB export did not finish")
