# pull the Node.js Docker image
FROM node:alpine

# create the directory inside the container
RUN mkdir -p /usr/src/myapp

# set as the working directory
WORKDIR /usr/src/myapp

# copy the package.json files from local machine to the workdir in container
COPY package*.json ./

# run npm install
RUN npm install

# copy the generated modules and all other files to the container
COPY . .

# run build as this is a production docker image
RUN npm run build

# Expose port
EXPOSE 8005

# the command that starts the app
CMD ["npm", "run", "start"]
