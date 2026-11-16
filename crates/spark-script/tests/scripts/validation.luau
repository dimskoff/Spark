-- Regression: numbers must be finite and files must stay inside the game folder.
-- Every call below must fail with a clear error; none may be silently accepted.

local nan = 0 / 0
local inf = 1 / 0

local function fails(what, fn, expect)
    local ok, err = pcall(fn)
    assert(not ok, what .. " was accepted")
    if expect then
        assert(string.find(tostring(err), expect, 1, true), what .. ": error was '" .. tostring(err) .. "', expected it to mention '" .. expect .. "'")
    end
end

function start()
    -- NaN / infinity in numbers
    fails("every(NaN)", function() every(nan, function() end) end, "finite")
    fails("after(NaN)", function() after(nan, function() end) end, "finite")
    fails("after(inf)", function() after(inf, function() end) end, "finite")
    fails("Mesh.cube(NaN)", function() Mesh.cube(nan) end, "finite")
    fails("Mesh.sphere(inf)", function() Mesh.sphere(inf) end, "finite")
    fails("Mesh.box(NaN)", function() Mesh.box(1, nan, 1) end, "finite")
    fails("camera.fov = NaN", function() camera.fov = nan end, "finite")
    fails("camera.x = inf", function() camera.x = inf end, "finite")
    fails("vec3 position NaN", function() spawn(Mesh.cube(), "red", { position = vec3(nan, 0, 0) }) end, "finite")
    fails("scale inf", function() spawn(Mesh.cube(), "red", { scale = inf }) end, "finite")
    fails("Color.rgb(NaN)", function() Color.rgb(nan, 0, 0) end, "finite")
    fails("sun intensity NaN", function() sun(vec3(0, -1, 0), "white", nan) end, "finite")
    fails("fog NaN", function() fog("white", nan, 10) end, "finite")

    -- ranges
    fails("fov 0", function() camera.fov = 0 end, "between 1 and 179")
    fails("fov 200", function() camera.fov = 200 end, "between 1 and 179")
    fails("near 0", function() camera.near = 0 end, "greater than 0")
    fails("far below near", function() camera.far = camera.near / 2 end, "greater than camera.near")
    fails("Mesh.cube(-1)", function() Mesh.cube(-1) end, "greater than 0")
    fails("Mesh.sphere(0)", function() Mesh.sphere(0) end, "greater than 0")
    fails("Mesh.box width", function() Mesh.box(0, 1, 1) end, "greater than 0")
    fails("body mass 0", function() spawn(Mesh.cube(), "red", { body = { mass = 0 } }) end, "greater than 0")
    fails("body friction -1", function() spawn(Mesh.cube(), "red", { body = { friction = -1 } }) end, "0 or more")
    camera.fov = 60

    local cube = spawn(Mesh.cube(), "red", { body = "dynamic" })
    fails("mass NaN", function() cube.mass = nan end, "finite")
    fails("velocity NaN", function() cube.velocity = vec3(0, nan, 0) end, "finite")
    fails("rotate NaN", function() cube:rotate_y(nan) end, "finite")
    fails("position inf", function() cube.position = vec3(inf, 0, 0) end, "finite")
    fails("physics.gravity NaN", function() physics.gravity = vec3(0, nan, 0) end, "finite")

    -- the object is untouched by the rejected writes
    assert(cube.position.x == cube.position.x, "position became NaN")

    -- paths outside the game folder
    fails("Texture.load absolute", function() Texture.load("/etc/passwd") end, "inside the game folder")
    fails("Texture.load ..", function() Texture.load("../../../etc/passwd") end, "inside the game folder")
    fails("Texture.load drive", function() Texture.load("C:/Windows/win.ini") end, "inside the game folder")
    fails("Texture.load UNC", function() Texture.load("\\\\server\\share\\x.png") end, "inside the game folder")
    fails("Model.load ..", function() Model.load("../x.glb") end, "inside the game folder")
    fails("Font.load absolute", function() Font.load("/usr/share/fonts/x.ttf") end, "inside the game folder")
    fails("sound.play ..", function() sound.play("../x.wav") end, "inside the game folder")
    fails("Shader.load ..", function() Shader.load("../x.wgsl") end, "inside the game folder")
    fails("require drive", function() require("./C:/Windows/win") end)

    quit()
end
