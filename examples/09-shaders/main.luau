-- A custom WGSL surface shader (shaders/toon.wgsl) and a post-process shader (shaders/outline.wgsl). See docs/SPARK_SHADERS.md.
local toon = Shader.load("shaders/toon.wgsl")
toon.bands = 3
local outline = Shader.load("shaders/outline.wgsl")
outline.color = Color.rgb(0.05, 0.04, 0.08)
outline.thickness = 1.5

function start()
    sun(vec3(-0.5, -1, -0.4), "white", 1.3)
    sky({ top = "#7fb0ff", bottom = "#f4f0e0" })
    spawn(Mesh.plane(20), "#7aa060")
    spawn(Mesh.sphere(1), "#e86a4a", { position = vec3(-1.6, 1, 0), shader = toon })
    spawn(Mesh.cube(1.6), "#4aa3e8", { position = vec3(1.6, 0.8, 0), shader = toon })
    graphics.post = { outline }
    camera.position = vec3(0, 3, 7)
    camera:look_at(vec3(0, 1, 0))
end